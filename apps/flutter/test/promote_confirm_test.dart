import 'package:agent_console/app/providers.dart';
import 'package:agent_console/core/net/ws_transport.dart';
import 'package:agent_console/core/pairing/mac_profile.dart';
import 'package:agent_console/core/pairing/mac_store.dart';
import 'package:agent_console/features/chat/chat_cache.dart';
import 'package:agent_console/features/inbox/inbox_notifier.dart';
import 'package:agent_console/features/inbox/inbox_screen.dart';
import 'package:agent_console/features/sessions/archive_store.dart';
import 'package:agent_console/features/sessions/sessions_notifier.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:flutter_test/flutter_test.dart';

import 'support/fake_ws.dart';
import 'support/harness.dart';

Map<String, Object?> action(String id, String label, {bool primary = false}) => <String, Object?>{'id': id, 'label': label, 'primary': primary};

Map<String, Object?> clusterItem(String id, String kind, String title, List<Map<String, Object?>> actions, {String detail = '', String? session}) => <String, Object?>{
      'id': id,
      'kind': kind,
      'title': title,
      'detail': detail,
      'createdMs': 1793541465000,
      'actions': actions,
      'state': 'open',
      'seen': false,
      if (session != null) 'source': <String, Object?>{'session': session},
    };

FakeWs mac(List<Map<String, Object?>> items) {
  return FakeWs(onSend: (FakeWs s, Map<String, Object?> m) {
    switch (m['op']) {
      case 'hello':
        s.reply(m['id']!, data: <String, Object?>{'version': 1});
      case 'sessions.list':
        s.reply(m['id']!, data: <String, Object?>{
          'sessions': <Object?>[sessionJson('s1')],
        });
      case 'subscribe':
        s.reply(m['id']!, data: <String, Object?>{'subscribed': true});
      case 'inbox.list':
        s.reply(m['id']!, data: <String, Object?>{'items': items, 'unread': items.length});
      case 'inbox.answer':
        s.reply(m['id']!, data: <String, Object?>{'answered': true, 'state': 'answered', 'answer': m['action']});
    }
  });
}

List<Map<String, Object?>> answers(FakeWs socket) => socket.sent.where((Map<String, Object?> m) => m['op'] == 'inbox.answer').toList();

Future<void> show(WidgetTester tester, FakeWs socket) async {
  final MemorySecretStore secrets = MemorySecretStore()
    ..values['macs.v1'] = encodeProfiles(<MacProfile>[testMac])
    ..values['macs.active'] = testMac.id;
  final ProviderContainer container = ProviderContainer(overrides: <Override>[
    secretStoreProvider.overrideWithValue(secrets),
    chatCacheProvider.overrideWithValue(MemoryChatCache()),
    archiveStoreProvider.overrideWithValue(MemoryArchiveStore()),
    wsConnectorProvider.overrideWithValue((Endpoint endpoint) async => socket),
    networkChangesProvider.overrideWithValue(const Stream<void>.empty()),
  ]);
  addTearDown(container.dispose);
  await tester.runAsync(() async {
    await connect(container);
    container.listen<InboxState>(inboxProvider, (InboxState? a, InboxState b) {});
    container.listen<SessionsState>(sessionsProvider, (SessionsState? a, SessionsState b) {});
    await until(() => container.read(inboxProvider).phase != InboxPhase.loading);
    await until(() => container.read(sessionsProvider).sessions.isNotEmpty);
  });
  await tester.pumpWidget(UncontrolledProviderScope(container: container, child: host(const Scaffold(body: InboxScreen()), screen: true)));
  await tester.pump();
}

Future<void> press(WidgetTester tester, String label) async {
  await tester.tap(find.text(label).first);
  await tester.pump(const Duration(milliseconds: 500));
}

Future<void> confirm(WidgetTester tester, FakeWs socket, String label) async {
  await tester.tap(find.descendant(of: find.byType(AlertDialog), matching: find.text(label)));
  await tester.runAsync(() => until(() => answers(socket).isNotEmpty));
  await tester.pump();
}

void main() {
  testWidgets('continue_from_this_copy_confirms', (WidgetTester tester) async {
    final FakeWs socket = mac(<Map<String, Object?>>[
      clusterItem(
        'waits-1',
        'session_waits',
        'Session s1 waits for m1, which has newer work in it',
        <Map<String, Object?>>[action('wait_for', 'Wait for m1', primary: true), action('continue_here', 'Continue From This Copy')],
        session: 's1',
      ),
    ]);
    await show(tester, socket);
    await press(tester, 'Continue From This Copy');
    expect(find.text('Continue Session s1 here?'), findsOneWidget);
    expect(find.text('The newer copy on the other machine is kept beside it.'), findsOneWidget);
    expect(answers(socket), isEmpty);

    await tester.tap(find.text('Cancel'));
    await tester.pump(const Duration(milliseconds: 500));
    expect(find.byType(AlertDialog), findsNothing);
    expect(answers(socket), isEmpty);

    await press(tester, 'Continue From This Copy');
    await confirm(tester, socket, 'Continue From This Copy');
    expect(answers(socket).single, containsPair('action', 'continue_here'));
    expect(answers(socket).single, containsPair('itemId', 'waits-1'));
  });

  testWidgets('wait for is answered without a question', (WidgetTester tester) async {
    final FakeWs socket = mac(<Map<String, Object?>>[
      clusterItem(
        'waits-1',
        'session_waits',
        'Session s1 waits for m1',
        <Map<String, Object?>>[action('wait_for', 'Wait for m1', primary: true), action('continue_here', 'Continue From This Copy')],
        session: 's1',
      ),
    ]);
    await show(tester, socket);
    await press(tester, 'Wait for m1');
    await tester.runAsync(() => until(() => answers(socket).isNotEmpty));
    expect(find.byType(AlertDialog), findsNothing);
    expect(answers(socket).single, containsPair('action', 'wait_for'));
  });

  testWidgets('move here, take back and move anyway each ask first', (WidgetTester tester) async {
    const String risk = "m5 can't be reached, so Notchling can't show that its work is safe on m1.";
    final FakeWs socket = mac(<Map<String, Object?>>[
      clusterItem('primary-unreachable:3', 'primary_unreachable', 'm5 is unreachable', <Map<String, Object?>>[action('move_here', 'Move to m1 Now', primary: true)]),
      clusterItem('primary-unreachable:3:confirm', 'primary_unreachable', 'Move anyway', <Map<String, Object?>>[action('move_anyway', 'Move Anyway')], detail: risk),
      clusterItem('back-1', 'primary_back', 'm1 is primary', <Map<String, Object?>>[action('take_back', 'Take Back'), action('keep', 'Keep m1')]),
    ]);
    await show(tester, socket);

    await press(tester, 'Move to m1 Now');
    expect(find.text('Move to this machine now?'), findsOneWidget);
    await tester.tap(find.text('Cancel'));
    await tester.pump(const Duration(milliseconds: 500));

    await press(tester, 'Move Anyway');
    expect(find.text('Move anyway?'), findsOneWidget);
    expect(find.descendant(of: find.byType(AlertDialog), matching: find.text(risk)), findsOneWidget);
    await tester.tap(find.text('Cancel'));
    await tester.pump(const Duration(milliseconds: 500));

    await press(tester, 'Take Back');
    expect(find.text('Take Back?'), findsOneWidget);
    await tester.tap(find.text('Cancel'));
    await tester.pump(const Duration(milliseconds: 500));
    expect(answers(socket), isEmpty);

    await press(tester, 'Take Back');
    await confirm(tester, socket, 'Take Back');
    expect(answers(socket).single, containsPair('action', 'take_back'));
  });

  testWidgets('no_primary and primary_unreachable offer the Machines popup, and keep has no question', (WidgetTester tester) async {
    final FakeWs socket = mac(<Map<String, Object?>>[
      clusterItem('no-primary', 'no_primary', 'No primary', const <Map<String, Object?>>[]),
      clusterItem('back-1', 'primary_back', 'm1 is primary', <Map<String, Object?>>[action('take_back', 'Take Back'), action('keep', 'Keep m1')]),
    ]);
    await show(tester, socket);
    expect(find.text('Choose a Machine'), findsOneWidget);
    await press(tester, 'Keep m1');
    await tester.runAsync(() => until(() => answers(socket).isNotEmpty));
    expect(find.byType(AlertDialog), findsNothing);
    expect(answers(socket).single, containsPair('action', 'keep'));
  });
}
