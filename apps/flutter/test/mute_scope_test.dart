// A mute belongs to one session on one computer: muting `s1` on B leaves `s1` on A loud (decision 0012). It is kept
// under the same storage scope as the chat cache and the archive, so a mute made before the phone followed several
// computers stays with the computer it was made for.
import 'dart:convert';

import 'package:agent_console/app/alerts.dart';
import 'package:agent_console/app/message_alerts.dart';
import 'package:agent_console/core/pairing/mac_profile.dart';
import 'package:agent_console/core/pairing/mac_store.dart';
import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/chat/chat_screen.dart';
import 'package:agent_console/features/macs/macs_notifier.dart';
import 'package:agent_console/features/macs/session_ref.dart';
import 'package:agent_console/features/sessions/sessions_notifier.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/alert_harness.dart';
import 'support/harness.dart';
import 'support/two_computers.dart';

/// A phone that already follows both computers: [scopes] is what the first run of the update saved.
MemorySecretStore stored({required MacProfile active, Map<String, String>? scopes, List<String> muted = const <String>[]}) => MemorySecretStore()
  ..values['macs.v1'] = encodeProfiles(<MacProfile>[macA, macB])
  ..values['macs.active'] = active.id
  ..values['macs.scopes'] = jsonEncode(scopes ?? <String, String>{macA.id: '', macB.id: macB.id})
  ..values['prefs.muted'] = jsonEncode(muted);

/// A reply arrives in `s1` on the computer in use.
void reply(ProviderContainer container) {
  pushSessions(container, <Session>[alertSession('s1', reply: 'm1')]);
  pushSessions(container, <Session>[alertSession('s1', reply: 'm2')]);
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  group('what a mute silences', () {
    test('s1 muted on B is still heard on A', () async {
      final (:ProviderContainer container, service: _) = await watchedComputers(active: macA, secrets: stored(active: macA));
      await container.read(alertPrefsProvider.notifier).setMuted('s1', muted: true, scope: macB.id);
      reply(container);
      expect(container.read(inAppAlertProvider)?.session, const SessionRef('aaaaaaaaaaaaaaaa', 's1'));
    });

    test('s1 muted on A is still heard on B', () async {
      final (:ProviderContainer container, service: _) = await watchedComputers(active: macB, secrets: stored(active: macB));
      await container.read(alertPrefsProvider.notifier).setMuted('s1', muted: true, scope: '');
      reply(container);
      expect(container.read(inAppAlertProvider)?.session, const SessionRef('bbbbbbbbbbbbbbbb', 's1'));
    });

    test('s1 muted on the computer in use is silent there', () async {
      final (:ProviderContainer container, service: _) = await watchedComputers(active: macB, secrets: stored(active: macB));
      await container.read(alertPrefsProvider.notifier).setMuted('s1', muted: true, scope: macB.id);
      reply(container);
      expect(container.read(inAppAlertProvider), isNull);
    });

    test('mutes are saved under the computer’s storage scope: bare for the one that kept the empty scope', () async {
      final MemorySecretStore secrets = stored(active: macA);
      final (:ProviderContainer container, service: _) = await watchedComputers(active: macA, secrets: secrets);
      await container.read(alertPrefsProvider.notifier).setMuted('s1', muted: true, scope: macB.id);
      await container.read(alertPrefsProvider.notifier).setMuted('s1', muted: true, scope: '');
      expect((jsonDecode(secrets.values['prefs.muted']!) as List<Object?>).toSet(), <String>{'bbbbbbbbbbbbbbbb/s1', 's1'});
    });
  });

  group('mutes saved before the phone followed several computers', () {
    test('belong to the computer that kept the bare keys, and not to the other', () async {
      final (container: ProviderContainer onA, service: _) = await watchedComputers(active: macA, secrets: stored(active: macA, muted: <String>['s1']));
      reply(onA);
      expect(onA.read(inAppAlertProvider), isNull, reason: 'the old mute is A’s');

      // The same phone, with B in use.
      final (container: ProviderContainer onB, service: _) = await watchedComputers(active: macB, secrets: stored(active: macB, muted: <String>['s1']));
      reply(onB);
      expect(onB.read(inAppAlertProvider)?.session, const SessionRef('bbbbbbbbbbbbbbbb', 's1'), reason: 'B never had it');
    });

    test('with no scopes saved yet, they go to the computer in use at the first run', () async {
      final MemorySecretStore secrets = MemorySecretStore()
        ..values['macs.v1'] = encodeProfiles(<MacProfile>[macA, macB])
        ..values['macs.active'] = macB.id
        ..values['prefs.muted'] = jsonEncode(<String>['s1']);
      final (:ProviderContainer container, service: _) = await watchedComputers(active: macB, secrets: secrets);
      expect(container.read(macsProvider).scopeOf(macB.id), '');
      reply(container);
      expect(container.read(inAppAlertProvider), isNull);
      await container.read(macsProvider.notifier).setActive(macA.id);
      expect(container.read(alertPrefsProvider).isMuted(container.read(macsProvider).scopeOf(macA.id), 's1'), isFalse);
    });
  });

  testWidgets('the bell in Chat mutes the session on its own computer only', (WidgetTester tester) async {
    final ProviderContainer container = pairedComputers(computerSocket(), computerSocket());
    container.read(macsProvider);
    await tester.runAsync(() => until(() => container.read(macsProvider).loaded));
    container.read(selectedSessionProvider.notifier).select(const SessionRef('bbbbbbbbbbbbbbbb', 's1'));
    await tester.pumpWidget(UncontrolledProviderScope(container: container, child: host(const Scaffold(body: ChatScreen()), screen: true)));
    await tester.runAsync(() => until(() => container.read(sessionsProvider).byId('s1') != null && container.read(alertPrefsProvider).loaded));
    await tester.pump(const Duration(milliseconds: 100));

    await tester.tap(find.byIcon(Icons.notifications_none));
    await tester.pump();
    expect(find.byIcon(Icons.notifications_off_outlined), findsOneWidget);
    expect(container.read(alertPrefsProvider).muted, <String>{'bbbbbbbbbbbbbbbb/s1'});

    // A's s1 is another session: not muted.
    container.read(selectedSessionProvider.notifier).select(const SessionRef('aaaaaaaaaaaaaaaa', 's1'));
    await tester.runAsync(() => until(() => container.read(sessionsProvider).byId('s1') != null));
    await tester.pump(const Duration(milliseconds: 100));
    expect(find.byIcon(Icons.notifications_none), findsOneWidget);
    expect(find.byIcon(Icons.notifications_off_outlined), findsNothing);

    await tester.tap(find.byIcon(Icons.notifications_none));
    await tester.pump();
    expect(container.read(alertPrefsProvider).muted, <String>{'bbbbbbbbbbbbbbbb/s1', 's1'});
    await tester.pumpWidget(const SizedBox.shrink());
  });
}
