// A takeover started on one computer ends on that computer, even when the user picked the other computer's
// session of the same id while it was running (decision 0012).
import 'package:agent_console/app/providers.dart';
import 'package:agent_console/core/net/ws_transport.dart';
import 'package:agent_console/core/pairing/mac_profile.dart';
import 'package:agent_console/core/pairing/mac_store.dart';
import 'package:agent_console/features/chat/chat_cache.dart';
import 'package:agent_console/features/chat/chat_screen.dart';
import 'package:agent_console/features/macs/macs_notifier.dart';
import 'package:agent_console/features/macs/session_ref.dart';
import 'package:agent_console/features/sessions/archive_store.dart';
import 'package:agent_console/features/sessions/sessions_notifier.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:flutter_test/flutter_test.dart';

import 'support/fake_ws.dart';
import 'support/harness.dart';

const MacProfile macA = MacProfile(
  id: 'aaaaaaaaaaaaaaaa',
  name: 'Mac A',
  hosts: <String>['10.0.0.1'],
  port: 47821,
  fingerprint: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  token: 'tok-a',
);

const MacProfile macB = MacProfile(
  id: 'bbbbbbbbbbbbbbbb',
  name: 'Linux B',
  hosts: <String>['10.0.0.2'],
  port: 47821,
  fingerprint: 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
  token: 'tok-b',
);

/// A computer whose `s1` is view-only and can be taken over; it answers a takeover only when the test says so.
FakeWs _computer() => FakeWs(onSend: (FakeWs socket, Map<String, Object?> message) {
      final Object id = message['id']!;
      switch (message['op']) {
        case 'hello':
          socket.reply(id, data: <String, Object?>{'version': 1});
        case 'sessions.list':
          socket.reply(id, data: <String, Object?>{
            'sessions': <Object?>[sessionJson('s1', state: 'done', control: 'viewOnly', takeover: <String, Object?>{'available': true})],
          });
        case 'chat.get':
          socket.reply(id, data: <String, Object?>{'messages': <Object?>[], 'hasMore': false});
        case 'subscribe':
          socket.reply(id, data: <String, Object?>{'subscribed': true});
        case 'commands.list':
          socket.reply(id, data: <String, Object?>{'commands': <Object?>[]});
      }
    });

void main() {
  testWidgets('switching computers while a takeover runs files the new session under the computer it ran on', (WidgetTester tester) async {
    final FakeWs a = _computer();
    final FakeWs b = _computer();
    final ProviderContainer container = ProviderContainer(overrides: <Override>[
      secretStoreProvider.overrideWithValue(MemorySecretStore()
        ..values['macs.v1'] = encodeProfiles(<MacProfile>[macA, macB])
        ..values['macs.active'] = macA.id),
      chatCacheProvider.overrideWithValue(MemoryChatCache()),
      archiveStoreProvider.overrideWithValue(MemoryArchiveStore()),
      wsConnectorProvider.overrideWithValue((Endpoint endpoint) async => endpoint.host == '10.0.0.2' ? b : a),
      networkChangesProvider.overrideWithValue(const Stream<void>.empty()),
    ]);
    addTearDown(container.dispose);
    container.read(macsProvider);
    await tester.runAsync(() => until(() => container.read(macsProvider).loaded));

    // B's s1 is open in Chat.
    container.read(selectedSessionProvider.notifier).select(const SessionRef('bbbbbbbbbbbbbbbb', 's1'));
    await tester.pumpWidget(UncontrolledProviderScope(container: container, child: host(const Scaffold(body: ChatScreen()), screen: true)));
    await tester.runAsync(() => until(() => container.read(sessionsProvider).byId('s1') != null));
    await tester.pump(const Duration(milliseconds: 100));

    // Take it over; the confirmation, then the request is on its way to B.
    await tester.tap(find.text('Tiếp quản'));
    await tester.pumpAndSettle();
    await tester.tap(find.descendant(of: find.byType(AlertDialog), matching: find.text('Tiếp quản')));
    await tester.pump();
    await tester.runAsync(() => until(() => b.sent.any((Map<String, Object?> m) => m['op'] == 'session.takeover')));

    // Meanwhile the user opens A's s1 (the same id), which makes A the computer in use.
    container.read(selectedSessionProvider.notifier).select(const SessionRef('aaaaaaaaaaaaaaaa', 's1'));
    await tester.pump(const Duration(milliseconds: 100));
    expect(container.read(macsProvider).activeId, macA.id);

    // B answers: its resumed session is on B.
    final Object requestId = b.sent.firstWhere((Map<String, Object?> m) => m['op'] == 'session.takeover')['id']!;
    b.reply(requestId, data: <String, Object?>{'sessionId': 'resumed-1', 'resumed': true});
    await tester.runAsync(() => until(() => container.read(selectedSessionProvider)?.id == 'resumed-1'));

    expect(container.read(selectedSessionProvider), const SessionRef('bbbbbbbbbbbbbbbb', 'resumed-1'), reason: 'filed under B, not under A');
    expect(container.read(macsProvider).activeId, macB.id);
    expect(a.sent.where((Map<String, Object?> m) => m['op'] == 'session.takeover'), isEmpty);
    await tester.pumpWidget(const SizedBox.shrink());
  });
}
