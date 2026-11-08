// Removing a computer deletes what the phone kept for it: its chat cache, archive, mutes and token (decision 0012).
// What belongs to the computers that stay is left alone.
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:agent_console/app/alerts.dart';
import 'package:agent_console/core/pairing/mac_profile.dart';
import 'package:agent_console/core/pairing/mac_store.dart';
import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/chat/chat_cache.dart';
import 'package:agent_console/features/macs/macs_notifier.dart';
import 'package:agent_console/features/sessions/archive_store.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/harness.dart';
import 'support/two_computers.dart';

ChatMessage _message(String text) => ChatMessage.fromJson(<String, Object?>{'id': 'm-$text', 'role': 'user', 'text': text, 'time': 0});

/// A phone that follows A (which kept the bare keys, scope '') and B (scope = its host id), with a cached chat,
/// a picture, an archived session and a muted session for each.
({ProviderContainer container, MemorySecretStore secrets, MemoryChatCache cache, MemoryArchiveStore archive}) phoneWithTwo({List<MacProfile>? macs}) {
  final MemorySecretStore secrets = MemorySecretStore()
    ..values['macs.v1'] = encodeProfiles(macs ?? <MacProfile>[macA, macB])
    ..values['macs.active'] = macA.id
    ..values['macs.scopes'] = jsonEncode(<String, String>{macA.id: '', macB.id: macB.id})
    ..values['prefs.keepalive'] = 'on'
    ..values['prefs.muted'] = jsonEncode(<String>['s1', 'bbbbbbbbbbbbbbbb/s1']);
  final MemoryChatCache cache = MemoryChatCache()
    ..messages['s1'] = <ChatMessage>[_message('on A')]
    ..messages['bbbbbbbbbbbbbbbb/s1'] = <ChatMessage>[_message('on B')]
    ..images['s1|img|320'] = Uint8List.fromList(<int>[1])
    ..images['bbbbbbbbbbbbbbbb/s1|img|320'] = Uint8List.fromList(<int>[2]);
  final MemoryArchiveStore archive = MemoryArchiveStore(<String, int>{'s1': 1, 'bbbbbbbbbbbbbbbb/s1': 2});
  final ProviderContainer container = pairedComputers(computerSocket(), computerSocket(), secrets: secrets, cache: cache, archive: archive);
  return (container: container, secrets: secrets, cache: cache, archive: archive);
}

Future<void> started(ProviderContainer container) async {
  container.read(macsProvider);
  container.listen<AlertPrefs>(alertPrefsProvider, (AlertPrefs? a, AlertPrefs b) {});
  container.listen<Map<String, int>>(archivedSessionsProvider, (Map<String, int>? a, Map<String, int> b) {});
  await until(() => container.read(macsProvider).loaded && container.read(alertPrefsProvider).loaded && container.read(archivedSessionsProvider).isNotEmpty);
}

void main() {
  group('removing a computer', () {
    test('B’s cache, archive, mutes and token go; A’s stay', () async {
      final (:ProviderContainer container, :MemorySecretStore secrets, :MemoryChatCache cache, :MemoryArchiveStore archive) = phoneWithTwo();
      await started(container);

      await container.read(macsProvider.notifier).remove(macB.id);

      expect(cache.messages.keys, <String>['s1'], reason: 'B’s cached chat is gone');
      expect(cache.images.keys, <String>['s1|img|320'], reason: 'and its pictures');
      expect(archive.saved, <String, int>{'s1': 1});
      expect(container.read(archivedSessionsProvider), <String, int>{'s1': 1});
      expect(container.read(alertPrefsProvider).muted, <String>{'s1'});
      expect((jsonDecode(secrets.values['prefs.muted']!) as List<Object?>), <Object?>['s1'], reason: 'saved, not only in memory');
      expect(secrets.values.values.where((String v) => v.contains('tok-b')), isEmpty, reason: 'B’s token is gone');
      expect(jsonDecode(secrets.values['macs.scopes']!), <String, Object?>{macA.id: ''});
      expect(secrets.values['prefs.keepalive'], 'on', reason: 'settings that aren’t B’s stay');
    });

    test('A, the computer with the bare keys, takes the bare keys with it; B’s stay', () async {
      final (:ProviderContainer container, :MemorySecretStore secrets, :MemoryChatCache cache, :MemoryArchiveStore archive) = phoneWithTwo();
      await started(container);

      await container.read(macsProvider.notifier).remove(macA.id);

      expect(cache.messages.keys, <String>['bbbbbbbbbbbbbbbb/s1']);
      expect(cache.images.keys, <String>['bbbbbbbbbbbbbbbb/s1|img|320']);
      expect(archive.saved, <String, int>{'bbbbbbbbbbbbbbbb/s1': 2});
      expect(container.read(alertPrefsProvider).muted, <String>{'bbbbbbbbbbbbbbbb/s1'});
      expect(secrets.values.values.where((String v) => v.contains('tok-a')), isEmpty, reason: 'A’s token is gone');
      expect(container.read(macsProvider).activeId, macB.id);
      expect(secrets.values['macs.active'], macB.id);
    });

    test('the last computer leaves no token and no “in use” entry behind', () async {
      final (:ProviderContainer container, :MemorySecretStore secrets, :MemoryChatCache cache, :MemoryArchiveStore archive) = phoneWithTwo(macs: <MacProfile>[macA]);
      await started(container);

      await container.read(macsProvider.notifier).remove(macA.id);

      expect(container.read(macsProvider).macs, isEmpty);
      expect(secrets.values.values.where((String v) => v.contains('tok-a')), isEmpty);
      expect(secrets.values.containsKey('macs.active'), isFalse);
      expect(cache.messages.keys, <String>['bbbbbbbbbbbbbbbb/s1'], reason: 'B was never paired here; its entries are not A’s to delete');
      expect(archive.saved, <String, int>{'bbbbbbbbbbbbbbbb/s1': 2});
    });
  });

  group('the chat cache on disk', () {
    late Directory root;
    late DiskChatCache disk;

    setUp(() {
      root = Directory.systemTemp.createTempSync('chat_cache_test');
      disk = DiskChatCache(() async => root);
    });
    tearDown(() => root.deleteSync(recursive: true));

    Future<void> fill() async {
      for (final String key in <String>['s1', 'bbbbbbbbbbbbbbbb/s1', 'cccccccccccccccc/s1']) {
        await disk.saveMessages(key, <ChatMessage>[_message(key)]);
        await disk.saveImage(key, 'img', 320, Uint8List.fromList(<int>[1, 2, 3]));
      }
    }

    test('removing a computer deletes its chats and pictures from disk, and only its', () async {
      await fill();
      await disk.deleteScope('bbbbbbbbbbbbbbbb');
      expect(await disk.loadMessages('bbbbbbbbbbbbbbbb/s1'), isEmpty);
      expect(await disk.loadImage('bbbbbbbbbbbbbbbb/s1', 'img', 320), isNull);
      expect((await disk.loadMessages('cccccccccccccccc/s1')).single.text, 'cccccccccccccccc/s1');
      expect(await disk.loadImage('cccccccccccccccc/s1', 'img', 320), isNotNull);
      expect((await disk.loadMessages('s1')).single.text, 's1');
      expect(await disk.loadImage('s1', 'img', 320), isNotNull);
    });

    test('the computer with the bare keys takes the bare entries; the others stay', () async {
      await fill();
      await disk.deleteScope('');
      expect(await disk.loadMessages('s1'), isEmpty);
      expect(await disk.loadImage('s1', 'img', 320), isNull);
      expect((await disk.loadMessages('bbbbbbbbbbbbbbbb/s1')).single.text, 'bbbbbbbbbbbbbbbb/s1');
      expect((await disk.loadMessages('cccccccccccccccc/s1')).single.text, 'cccccccccccccccc/s1');
    });

    test('the same session id on two computers is two files, and nothing is left of a deleted one', () async {
      await fill();
      List<String> files() => <String>[for (final FileSystemEntity e in root.listSync(recursive: true)) e.path];
      expect(files().where((String path) => path.contains('bbbbbbbbbbbbbbbb')), isNotEmpty, reason: 'a computer’s files live under its own folder');
      await disk.deleteScope('bbbbbbbbbbbbbbbb');
      expect(files().where((String path) => path.contains('bbbbbbbbbbbbbbbb')), isEmpty);
      expect(files().where((String path) => path.contains('cccccccccccccccc')), isNotEmpty);
    });
  });
}
