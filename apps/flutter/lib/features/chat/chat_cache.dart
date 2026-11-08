import 'dart:convert';
import 'dart:io';

import 'package:crypto/crypto.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:path_provider/path_provider.dart';

import '../../core/protocol/models.dart';
import '../macs/session_ref.dart';

/// How many messages per session stay on the phone.
const int cachedMessageLimit = 100;

/// The last messages of each session and the thumbnails of their pictures, kept on the phone so a chat opens
/// at once and only what is newer has to be fetched.
abstract interface class ChatCache {
  Future<List<ChatMessage>> loadMessages(String sessionId);
  Future<void> saveMessages(String sessionId, List<ChatMessage> messages);
  Future<Uint8List?> loadImage(String sessionId, String imageId, int width);
  Future<void> saveImage(String sessionId, String imageId, int width, Uint8List jpeg);

  /// Forgets everything kept for the computer whose storage scope is [scope] (see `storageKey`), when it is removed.
  Future<void> deleteScope(String scope);
}

/// Files under the app's support folder. A cache that cannot be read or written is logged and treated as empty:
/// the chat still works from the network.
class DiskChatCache implements ChatCache {
  DiskChatCache([Future<Directory> Function()? root]) : _root = root ?? getApplicationSupportDirectory;
  final Future<Directory> Function() _root;

  String _key(String value) => sha1.convert(utf8.encode(value)).toString().substring(0, 24);

  /// Where a session's files go. File names are hashes, which can't be searched for a computer's entries, so a
  /// computer other than the first (a key `<host>/<id>`) keeps its files under its own folder and removing it
  /// deletes that folder. A bare key (the computer a phone had before it followed several) keeps the flat layout.
  Future<({String dir, String name})> _place(String kind, String sessionId) async {
    final String base = (await _root()).path;
    final SessionRef? on = SessionRef.parse(sessionId);
    return on == null ? (dir: '$base/$kind', name: _key(sessionId)) : (dir: '$base/computers/${on.host}/$kind', name: _key(on.id));
  }

  Future<File> _messagesFile(String sessionId) async {
    final ({String dir, String name}) at = await _place('chat_cache', sessionId);
    return File('${at.dir}/${at.name}.json');
  }

  Future<File> _imageFile(String sessionId, String imageId, int width) async {
    final ({String dir, String name}) at = await _place('chat_images', sessionId);
    return File('${at.dir}/${at.name}/${_key(imageId)}-$width.jpg');
  }

  @override
  Future<List<ChatMessage>> loadMessages(String sessionId) async {
    try {
      final File file = await _messagesFile(sessionId);
      if (!file.existsSync()) return const <ChatMessage>[];
      final Object? decoded = jsonDecode(await file.readAsString());
      if (decoded is! List<Object?>) return const <ChatMessage>[];
      return <ChatMessage>[for (final Object? item in decoded) ChatMessage.fromJson((item! as Map<Object?, Object?>).cast<String, Object?>())];
    } on Object catch (error) {
      debugPrint('ChatCache.loadMessages($sessionId): $error');
      return const <ChatMessage>[];
    }
  }

  @override
  Future<void> saveMessages(String sessionId, List<ChatMessage> messages) async {
    try {
      final File file = await _messagesFile(sessionId);
      await file.parent.create(recursive: true);
      final List<ChatMessage> tail = messages.length > cachedMessageLimit ? messages.sublist(messages.length - cachedMessageLimit) : messages;
      final File scratch = File('${file.path}.tmp');
      await scratch.writeAsString(jsonEncode(<Object?>[for (final ChatMessage m in tail) m.toJson()]), flush: true);
      await scratch.rename(file.path);
    } on Object catch (error) {
      debugPrint('ChatCache.saveMessages($sessionId): $error');
    }
  }

  @override
  Future<Uint8List?> loadImage(String sessionId, String imageId, int width) async {
    try {
      final File file = await _imageFile(sessionId, imageId, width);
      return file.existsSync() ? await file.readAsBytes() : null;
    } on Object catch (error) {
      debugPrint('ChatCache.loadImage($sessionId, $imageId): $error');
      return null;
    }
  }

  @override
  Future<void> deleteScope(String scope) async {
    final String base = (await _root()).path;
    // The first computer's entries are the flat ones; any other's are its folder.
    final List<String> paths = scope.isEmpty ? <String>['$base/chat_cache', '$base/chat_images'] : <String>['$base/computers/$scope'];
    for (final String path in paths) {
      try {
        final Directory dir = Directory(path);
        if (dir.existsSync()) await dir.delete(recursive: true);
      } on Object catch (error) {
        debugPrint('ChatCache.deleteScope($scope): $error');
      }
    }
  }

  @override
  Future<void> saveImage(String sessionId, String imageId, int width, Uint8List jpeg) async {
    try {
      final File file = await _imageFile(sessionId, imageId, width);
      await file.parent.create(recursive: true);
      await file.writeAsBytes(jpeg, flush: true);
    } on Object catch (error) {
      debugPrint('ChatCache.saveImage($sessionId, $imageId): $error');
    }
  }
}

/// Overridden in tests with an in-memory cache.
final Provider<ChatCache> chatCacheProvider = Provider<ChatCache>((Ref ref) => DiskChatCache());

/// Keeps everything in memory. For tests, and as a stand-in where no disk is wanted.
class MemoryChatCache implements ChatCache {
  final Map<String, List<ChatMessage>> messages = <String, List<ChatMessage>>{};
  final Map<String, Uint8List> images = <String, Uint8List>{};

  @override
  Future<List<ChatMessage>> loadMessages(String sessionId) async => messages[sessionId] ?? const <ChatMessage>[];

  @override
  Future<void> saveMessages(String sessionId, List<ChatMessage> value) async =>
      messages[sessionId] = value.length > cachedMessageLimit ? value.sublist(value.length - cachedMessageLimit) : value;

  @override
  Future<Uint8List?> loadImage(String sessionId, String imageId, int width) async => images['$sessionId|$imageId|$width'];

  @override
  Future<void> saveImage(String sessionId, String imageId, int width, Uint8List jpeg) async => images['$sessionId|$imageId|$width'] = jpeg;

  @override
  Future<void> deleteScope(String scope) async {
    messages.removeWhere((String key, List<ChatMessage> _) => keyInScope(scope, key));
    // An image is `<session key>|<image id>|<width>`.
    images.removeWhere((String key, Uint8List _) => keyInScope(scope, key.split('|').first));
  }
}

/// One computer's view of the cache: its session ids carry its scope (decision 0012), so `s1` on two computers
/// are two entries. The empty scope passes ids through, which is what a phone paired before 0012 has on disk.
class ScopedChatCache implements ChatCache {
  const ScopedChatCache(this._inner, this.scope);
  final ChatCache _inner;
  final String scope;

  String _key(String sessionId) => scope.isEmpty ? sessionId : '$scope/$sessionId';

  @override
  Future<List<ChatMessage>> loadMessages(String sessionId) => _inner.loadMessages(_key(sessionId));

  @override
  Future<void> saveMessages(String sessionId, List<ChatMessage> messages) => _inner.saveMessages(_key(sessionId), messages);

  @override
  Future<Uint8List?> loadImage(String sessionId, String imageId, int width) => _inner.loadImage(_key(sessionId), imageId, width);

  @override
  Future<void> saveImage(String sessionId, String imageId, int width, Uint8List jpeg) =>
      _inner.saveImage(_key(sessionId), imageId, width, jpeg);

  @override
  Future<void> deleteScope(String scope) => _inner.deleteScope(scope);
}
