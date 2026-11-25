import 'package:agent_console/app/providers.dart';
import 'package:agent_console/core/net/bridge_client.dart';
import 'package:agent_console/core/net/ws_transport.dart';
import 'package:agent_console/core/pairing/mac_profile.dart';
import 'package:agent_console/core/pairing/mac_store.dart';
import 'package:agent_console/features/chat/chat_cache.dart';
import 'package:agent_console/features/connection/connection.dart';
import 'package:agent_console/features/machines/machines_models.dart';
import 'package:agent_console/features/machines/machines_sheet.dart';
import 'package:agent_console/features/sessions/archive_store.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:flutter_test/flutter_test.dart';

import 'support/fake_ws.dart';
import 'support/harness.dart';

const String m1Id = '1111111111111111';
const String m3Id = '3333333333333333';
const String piId = '4444444444444444';

final DateTime copyTime = DateTime(2026, 11, 14, 14, 2);

Map<String, Object?> machine(String id, String name, {bool canBeMaster = true, bool ready = true, bool online = true, DateTime? set}) => <String, Object?>{
      'id': id,
      'name': name,
      'online': online,
      'canBeMaster': canBeMaster,
      'ready': ready,
      'canResume': canBeMaster && ready && set != null,
      'setAt': ?set?.millisecondsSinceEpoch,
    };

class _Fallback extends ConnectionStatusNotifier {
  @override
  ConnectionStatus build() => const ConnectionStatus(LinkPhase.connected, via: 'm1', primaryName: 'm5');
}

List<Map<String, Object?>> standardMachines() => <Map<String, Object?>>[
      machine(m1Id, 'm1', set: copyTime),
      machine(m3Id, 'm3'),
      machine(piId, 'pi', canBeMaster: false),
      machine('5555555555555555', 'm5', online: false),
    ];

FakeWs mac({List<Map<String, Object?>>? machines, bool oldMac = false, List<Map<String, Object?>> promotes = const <Map<String, Object?>>[]}) {
  int promoted = 0;
  return FakeWs(onSend: (FakeWs s, Map<String, Object?> m) {
    switch (m['op']) {
      case 'hello':
        s.reply(m['id']!, data: <String, Object?>{'version': 1});
      case 'cluster.machines':
        if (oldMac) {
          s.fail(m['id']!, 'invalid', 'No such op "cluster.machines".');
        } else {
          s.reply(m['id']!, data: <String, Object?>{'machines': machines ?? standardMachines()});
        }
      case 'cluster.promote':
        final Map<String, Object?> answer = promotes.isEmpty ? <String, Object?>{'step': 'bump'} : promotes[promoted.clamp(0, promotes.length - 1)];
        promoted++;
        s.reply(m['id']!, data: answer);
    }
  });
}

ProviderContainer containerFor(FakeWs socket) {
  final MemorySecretStore secrets = MemorySecretStore()
    ..values['macs.v1'] = encodeProfiles(<MacProfile>[testMac])
    ..values['macs.active'] = testMac.id;
  final ProviderContainer container = ProviderContainer(overrides: <Override>[
    secretStoreProvider.overrideWithValue(secrets),
    chatCacheProvider.overrideWithValue(MemoryChatCache()),
    archiveStoreProvider.overrideWithValue(MemoryArchiveStore()),
    wsConnectorProvider.overrideWithValue((Endpoint endpoint) async => socket),
    networkChangesProvider.overrideWithValue(const Stream<void>.empty()),
    connectionStatusProvider.overrideWith(_Fallback.new),
  ]);
  addTearDown(container.dispose);
  return container;
}

List<Map<String, Object?>> promoteRequests(FakeWs socket) => socket.sent.where((Map<String, Object?> m) => m['op'] == 'cluster.promote').toList();

Future<void> openSheet(WidgetTester tester, FakeWs socket, {bool expectSheet = true}) async {
  final ProviderContainer container = containerFor(socket);
  await tester.runAsync(() => connect(container));
  await tester.pumpWidget(UncontrolledProviderScope(
    container: container,
    child: host(
      Scaffold(
        body: Center(
          child: Consumer(
            builder: (BuildContext context, WidgetRef ref, Widget? _) => TextButton(onPressed: () => showMachinesSheet(context, ref), child: const Text('open')),
          ),
        ),
      ),
      screen: true,
    ),
  ));
  await tester.tap(find.text('open'));
  await tester.runAsync(() => until(() => socket.sent.any((Map<String, Object?> m) => m['op'] == 'cluster.machines')));
  await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 50)));
  await tester.pump();
  await tester.pump(const Duration(milliseconds: 500));
}

Future<void> settle(WidgetTester tester, FakeWs socket, {required int promotes}) async {
  await tester.runAsync(() => until(() => promoteRequests(socket).length >= promotes));
  await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 50)));
  await tester.pump();
  await tester.pump(const Duration(milliseconds: 500));
}

void main() {
  testWidgets('popup_lists_online_machines_with_snapshot_time', (WidgetTester tester) async {
    await openSheet(tester, mac());
    expect(find.text('Choose a Machine'), findsOneWidget);
    expect(find.text('m1'), findsOneWidget);
    expect(find.text('m3'), findsOneWidget);
    expect(find.text('pi'), findsOneWidget);
    expect(find.text('m5'), findsNothing);
    expect(find.text('Can resume old sessions · copy from ${clockText(copyTime)}'), findsOneWidget);
    expect(clockText(copyTime), '14:02');
  });

  testWidgets('machine_without_set_says_controls_jobs_only', (WidgetTester tester) async {
    await openSheet(tester, mac());
    expect(find.text('Controls jobs only'), findsOneWidget);
    final Finder row = find.ancestor(of: find.text('m3'), matching: find.byType(InkWell));
    expect(find.descendant(of: row, matching: find.text('Controls jobs only')), findsOneWidget);
  });

  testWidgets('not_can_be_master_isnt_pickable', (WidgetTester tester) async {
    final FakeWs socket = mac();
    await openSheet(tester, socket);
    expect(find.text('Can Be Master is off'), findsOneWidget);
    await tester.tap(find.text('pi'));
    await tester.pump(const Duration(milliseconds: 500));
    expect(find.byType(AlertDialog), findsNothing);
    expect(promoteRequests(socket), isEmpty);
    final InkWell ink = tester.widget<InkWell>(find.ancestor(of: find.text('pi'), matching: find.byType(InkWell)));
    expect(ink.onTap, isNull);
  });

  testWidgets('move_asks_before_sending', (WidgetTester tester) async {
    final FakeWs socket = mac();
    await openSheet(tester, socket);
    await tester.tap(find.text('m1'));
    await tester.pump(const Duration(milliseconds: 500));
    expect(find.text('Move to m1 now?'), findsOneWidget);
    expect(find.text('Sessions continue there from 14:02. Work m5 did after that is kept on m5.'), findsOneWidget);
    expect(promoteRequests(socket), isEmpty);
    await tester.tap(find.text('Move'));
    await settle(tester, socket, promotes: 1);
    expect(promoteRequests(socket).single, containsPair('node', m1Id));
    expect(promoteRequests(socket).single.containsKey('confirmed'), isFalse);
    expect(find.text('Moving to m1. Sessions continue there soon.'), findsOneWidget);
  });

  testWidgets('cancel_sends_nothing', (WidgetTester tester) async {
    final FakeWs socket = mac();
    await openSheet(tester, socket);
    await tester.tap(find.text('m1'));
    await tester.pump(const Duration(milliseconds: 500));
    await tester.tap(find.text('Cancel'));
    await tester.pump(const Duration(milliseconds: 500));
    await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 50)));
    expect(find.byType(AlertDialog), findsNothing);
    expect(promoteRequests(socket), isEmpty);
  });

  testWidgets('relayed_answer_says_nothing_moved', (WidgetTester tester) async {
    final FakeWs socket = mac(promotes: <Map<String, Object?>>[
      <String, Object?>{'relayed': true, 'text': 'm5 is reachable through m1; nothing moved'},
    ]);
    await openSheet(tester, socket);
    await tester.tap(find.text('m1'));
    await tester.pump(const Duration(milliseconds: 500));
    await tester.tap(find.text('Move'));
    await settle(tester, socket, promotes: 1);
    expect(find.text('m5 is reachable through m1; nothing moved'), findsOneWidget);
    expect(find.textContaining('Moving to'), findsNothing);
  });

  testWidgets('needs_confirm_shows_the_risk_text', (WidgetTester tester) async {
    const String risk = "m5 can't be reached, so Notchling can't show that its work is safe on m1. Work m5 did after 14:02 stays on m5.";
    final FakeWs socket = mac(promotes: <Map<String, Object?>>[
      <String, Object?>{'needsConfirm': risk},
      <String, Object?>{'step': 'bump'},
    ]);
    await openSheet(tester, socket);
    await tester.tap(find.text('m1'));
    await tester.pump(const Duration(milliseconds: 500));
    await tester.tap(find.text('Move'));
    await settle(tester, socket, promotes: 1);
    expect(find.text(risk), findsOneWidget);
    expect(find.text('Move Anyway'), findsOneWidget);
    expect(promoteRequests(socket), hasLength(1));
    await tester.tap(find.text('Move Anyway'));
    await settle(tester, socket, promotes: 2);
    expect(promoteRequests(socket).last, containsPair('confirmed', true));
    expect(promoteRequests(socket).last, containsPair('node', m1Id));
  });

  testWidgets('an older Mac without cluster.machines shows no popup and breaks nothing', (WidgetTester tester) async {
    final FakeWs socket = mac(oldMac: true);
    await openSheet(tester, socket);
    expect(find.text('Choose a Machine'), findsNothing);
    expect(find.byType(BottomSheet), findsNothing);
  });

  test('the models read the machine fixtures', () {
    final PromoteMachine withSet = PromoteMachine.fromJson(machine(m1Id, 'm1', set: copyTime));
    expect(withSet.canResume, isTrue);
    expect(withSet.setAt, copyTime);
    final PromoteMachine without = PromoteMachine.fromJson(machine(m3Id, 'm3'));
    expect(without.setAt, isNull);
    expect(PromoteMachine.fromJson(machine(piId, 'pi', canBeMaster: false)).pickable, isFalse);
  });
}
