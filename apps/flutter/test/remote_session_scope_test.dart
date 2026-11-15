import 'dart:convert';
import 'dart:io';

import 'package:agent_console/app/alerts.dart';
import 'package:agent_console/app/providers.dart';
import 'package:agent_console/core/pairing/mac_store.dart';
import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/chat/chat_cache.dart';
import 'package:agent_console/features/macs/session_ref.dart';
import 'package:agent_console/features/sessions/archive_store.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:flutter_test/flutter_test.dart';

import 'support/harness.dart';

const String remote = '0123456789abcdef/6f1c2d3e-0000-4000-8000-000000000001';

ProviderContainer prefs(MemorySecretStore secrets) {
  final ProviderContainer container = ProviderContainer(
    overrides: <Override>[
      secretStoreProvider.overrideWithValue(secrets),
      archiveStoreProvider.overrideWithValue(MemoryArchiveStore()),
    ],
  );
  addTearDown(container.dispose);
  return container;
}

void main() {
  test('a remote session id muted in the empty scope is muted there, and survives a reload', () async {
    final MemorySecretStore secrets = MemorySecretStore();
    final ProviderContainer container = prefs(secrets);
    await container.read(alertPrefsProvider.notifier).setMuted(remote, muted: true);
    final AlertPrefs saved = container.read(alertPrefsProvider);
    expect(saved.isMuted('', remote), isTrue);
    expect(saved.mutedIn(''), contains(remote), reason: 'its alerts are silenced');
    expect(saved.mutedIn('bbbbbbbbbbbbbbbb'), isEmpty);
    expect(jsonDecode(secrets.values['prefs.muted']!) as List<Object?>, <String>['/$remote']);

    final ProviderContainer again = prefs(secrets);
    again.read(alertPrefsProvider);
    await until(() => again.read(alertPrefsProvider).loaded);
    expect(again.read(alertPrefsProvider).mutedIn(''), contains(remote));

    await again.read(alertPrefsProvider.notifier).setMuted(remote, muted: false);
    expect(again.read(alertPrefsProvider).isMuted('', remote), isFalse);
  });

  test('a plain session and an old bare mute keep working beside a remote one', () async {
    final MemorySecretStore secrets = MemorySecretStore()..values['prefs.muted'] = jsonEncode(<String>['s1', 'bbbbbbbbbbbbbbbb/s1']);
    final ProviderContainer container = prefs(secrets);
    container.read(alertPrefsProvider);
    await until(() => container.read(alertPrefsProvider).loaded);
    await container.read(alertPrefsProvider.notifier).setMuted(remote, muted: true);
    expect(container.read(alertPrefsProvider).mutedIn(''), <String>{'s1', remote});
    expect(container.read(alertPrefsProvider).mutedIn('bbbbbbbbbbbbbbbb'), <String>{'s1'});
  });

  test('removing a scope takes the remote mutes of the empty scope and nothing of another', () async {
    final MemorySecretStore secrets = MemorySecretStore();
    final ProviderContainer container = prefs(secrets);
    final AlertPrefsNotifier notifier = container.read(alertPrefsProvider.notifier);
    await notifier.setMuted(remote, muted: true);
    await notifier.setMuted('s1', muted: true, scope: 'bbbbbbbbbbbbbbbb');
    await notifier.removeScope('bbbbbbbbbbbbbbbb');
    expect(container.read(alertPrefsProvider).muted, <String>{'/$remote'});
    await notifier.removeScope('');
    expect(container.read(alertPrefsProvider).muted, isEmpty);
  });

  test('the key of a remote session is in the empty scope and in no computer’s', () {
    final String key = storageKey('', remote);
    expect(keyInScope('', key), isTrue);
    expect(keyInScope('0123456789abcdef', key), isFalse);
    expect(keyInScope('', '0123456789abcdef/s1'), isFalse, reason: 'that one is a computer’s');
    expect(storageKey('0123456789abcdef', 's1'), '0123456789abcdef/s1');
    expect(storageKey('', 's1'), 's1');
  });

  test('a remote session’s archive entry is found in the empty scope', () {
    final DateTime at = DateTime(2026, 11, 2);
    final Session session = Session(
      id: remote,
      name: 'n',
      project: 'p',
      cwd: '/w',
      entrypoint: 'job',
      state: SessionState.done,
      control: SessionControl.job,
      lastActivity: at,
    );
    expect(hiddenSessionIdsIn(<String, int>{storageKey('', remote): at.millisecondsSinceEpoch}, <Session>[session], ''), <String>{remote});
  });

  test('the cache keeps a remote session under the empty scope’s own folders, so removing the scope removes it', () async {
    final Directory root = Directory.systemTemp.createTempSync('remote-scope');
    addTearDown(() => root.deleteSync(recursive: true));
    final ChatCache cache = ScopedChatCache(DiskChatCache(() async => root), '');
    await cache.saveMessages(remote, const <ChatMessage>[]);
    expect(Directory('${root.path}/computers').existsSync(), isFalse, reason: 'not filed as another computer');
    expect(Directory('${root.path}/chat_cache').existsSync(), isTrue);
    await cache.deleteScope('');
    expect(Directory('${root.path}/chat_cache').existsSync(), isFalse);
  });
}
