import 'dart:convert';
import 'dart:io';

import 'package:crypto/crypto.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:path_provider/path_provider.dart';

import '../../core/protocol/models.dart';

/// How many messages per session stay on the phone.
const int cachedMessageLimit = 100;

/// The last messages of each session and the thumbnails of their pictures, kept on the phone so a chat opens
/// at once and only what is newer has to be fetched.
abstract interface class ChatCache {
  Future<List<ChatMessage>> loadMessages(String sessionId);
  Future<void> saveMessages(String sessionId, List<ChatMessage> messages);
  Future<Uint8List?> loadImage(String sessionId, String imageId, int width);
  Future<void> saveImage(String sessionId, String imageId, int width, Uint8List jpeg);
}

/// Files under the app's support folder. A cache that cannot be read or written is logged and treated as empty:
/// the chat still works from the network.
class DiskChatCache implements ChatCache {
  DiskChatCache([Future<Directory> Function()? root]) : _root = root ?? getApplicationSupportDirectory;
  final Future<Directory> Function() _root;

  String _key(String value) => sha1.convert(utf8.encode(value)).toString().substring(0, 24);

  Future<File> _messagesFile(String sessionId) async => File('${(await _root()).path}/chat_cache/${_key(sessionId)}.json');
  Future<File> _imageFile(String sessionId, String imageId, int width) async =>
      File('${(await _root()).path}/chat_images/${_key(sessionId)}/${_key(imageId)}-$width.jpg');

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
}
