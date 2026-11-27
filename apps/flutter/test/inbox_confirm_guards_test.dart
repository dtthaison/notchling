import 'package:agent_console/app/providers.dart';
import 'package:agent_console/core/net/ws_transport.dart';
import 'package:agent_console/core/pairing/mac_profile.dart';
import 'package:agent_console/core/pairing/mac_store.dart';
import 'package:agent_console/app/widgets/common.dart';
import 'package:agent_console/features/chat/chat_cache.dart';
import 'package:agent_console/features/inbox/inbox_notifier.dart';
import 'package:agent_console/features/inbox/inbox_screen.dart';
import 'package:agent_console/features/macs/macs_notifier.dart';
import 'package:agent_console/features/sessions/archive_store.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:flutter_test/flutter_test.dart';

import 'promote_confirm_test.dart' hide main;
import 'support/fake_ws.dart';
import 'support/harness.dart';

const MacProfile otherMac = MacProfile(
  id: 'n',
  name: 'Mac khác',
  hosts: <String>['10.0.0.2'],
  port: 47821,
  fingerprint: '20d3c003d553a68bd969f5663f4b747be987a2874fe13bb025fe07710b829445',
  token: 'tok2',
);

List<Map<String, Object?>> moveItems() => <Map<String, Object?>>[
      clusterItem('primary-unreachable:3', 'primary_unreachable', 'm5 is unreachable', <Map<String, Object?>>[action('move_here', 'Move to m1 Now', primary: true)]),
    ];

Future<ProviderContainer> showTwo(WidgetTester tester, FakeWs first, FakeWs second) async {
  final MemorySecretStore secrets = MemorySecretStore()
    ..values['macs.v1'] = encodeProfiles(<MacProfile>[testMac, otherMac])
    ..values['macs.active'] = testMac.id;
  final ProviderContainer container = ProviderContainer(overrides: <Override>[
    secretStoreProvider.overrideWithValue(secrets),
    chatCacheProvider.overrideWithValue(MemoryChatCache()),
    archiveStoreProvider.overrideWithValue(MemoryArchiveStore()),
    wsConnectorProvider.overrideWithValue((Endpoint endpoint) async => endpoint.host == '10.0.0.2' ? second : first),
    networkChangesProvider.overrideWithValue(const Stream<void>.empty()),
  ]);
  addTearDown(container.dispose);
  await tester.runAsync(() async {
    await connect(container);
    container.listen<InboxState>(inboxProvider, (InboxState? a, InboxState b) {});
    await until(() => container.read(inboxProvider).phase != InboxPhase.loading);
  });
  await tester.pumpWidget(UncontrolledProviderScope(container: container, child: host(const Scaffold(body: InboxScreen()), screen: true)));
  await tester.pump();
  return container;
}

void main() {
  testWidgets('a double tap on an action asks once and answers once', (WidgetTester tester) async {
    final FakeWs socket = mac(moveItems());
    await show(tester, socket);
    final VoidCallback? start = tester.widget<PrimaryButton>(find.byType(PrimaryButton)).onPressed;
    start!();
    start();
    await tester.pump(const Duration(milliseconds: 500));
    expect(find.byType(AlertDialog), findsOneWidget);

    final VoidCallback? yes = tester.widget<TextButton>(find.widgetWithText(TextButton, 'Move to m1 Now')).onPressed;
    yes!();
    yes();
    await tester.runAsync(() => until(() => answers(socket).isNotEmpty));
    await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 100)));
    await tester.pump(const Duration(milliseconds: 500));
    expect(answers(socket), hasLength(1));
    expect(find.byType(InboxScreen), findsOneWidget);
  });

  testWidgets('an answer confirmed after another computer became active is not sent', (WidgetTester tester) async {
    final FakeWs first = mac(moveItems());
    final FakeWs second = mac(moveItems());
    final ProviderContainer container = await showTwo(tester, first, second);
    await tester.tap(find.text('Move to m1 Now').first);
    await tester.pump(const Duration(milliseconds: 500));
    expect(find.byType(AlertDialog), findsOneWidget);
    await tester.runAsync(() => container.read(macsProvider.notifier).setActive('n'));
    await tester.pump();
    await tester.tap(find.descendant(of: find.byType(AlertDialog), matching: find.text('Move to m1 Now')));
    await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 150)));
    await tester.pump(const Duration(milliseconds: 500));
    expect(answers(first), isEmpty);
    expect(answers(second), isEmpty);
  });
}
