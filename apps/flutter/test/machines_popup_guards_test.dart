import 'package:agent_console/app/notifications.dart';
import 'package:agent_console/app/providers.dart';
import 'package:agent_console/core/net/bridge_client.dart';
import 'package:agent_console/core/net/ws_transport.dart';
import 'package:agent_console/core/pairing/mac_profile.dart';
import 'package:agent_console/core/pairing/mac_store.dart';
import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/chat/chat_cache.dart';
import 'package:agent_console/features/connection/connection.dart';
import 'package:agent_console/features/inbox/inbox_notifier.dart';
import 'package:agent_console/features/machines/machines_notifier.dart';
import 'package:agent_console/features/machines/machines_sheet.dart';
import 'package:agent_console/features/macs/macs_notifier.dart';
import 'package:agent_console/features/sessions/archive_store.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:flutter_test/flutter_test.dart';

import 'machines_popup_test.dart' hide main;
import 'support/fake_ws.dart';
import 'support/harness.dart';

const ConnectionStatus fallback = ConnectionStatus(LinkPhase.connected, via: 'm1', primaryName: 'm5');
const ConnectionStatus plain = ConnectionStatus(LinkPhase.connected);
const ConnectionStatus reachedPrimary = ConnectionStatus(LinkPhase.connected, via: 'm1');

const MacProfile otherMac = MacProfile(
  id: 'n',
  name: 'Mac khác',
  hosts: <String>['10.0.0.2'],
  port: 47821,
  fingerprint: '20d3c003d553a68bd969f5663f4b747be987a2874fe13bb025fe07710b829445',
  token: 'tok2',
);

class StatusProbe extends ConnectionStatusNotifier {
  ConnectionStatus value = plain;

  @override
  ConnectionStatus build() {
    ref.watch(bridgeClientProvider);
    return value;
  }

  void put(ConnectionStatus next) {
    value = next;
    state = next;
  }
}

class InboxProbe extends InboxNotifier {
  @override
  InboxState build() => const InboxState(phase: InboxPhase.ready);

  void put(List<InboxItem> items) => state = InboxState(phase: InboxPhase.ready, items: items);
}

InboxItem outage(String id, String kind) => InboxItem.fromJson(<String, Object?>{
      'id': id,
      'kind': kind,
      'title': 'm5 is unreachable',
      'detail': '',
      'createdMs': 1,
      'actions': <Object?>[],
      'state': 'open',
    });

int machineRequests(FakeWs socket) => socket.sent.where((Map<String, Object?> m) => m['op'] == 'cluster.machines').length;

class Rig {
  Rig(this.container, this.first, this.second);

  final ProviderContainer container;
  final FakeWs first;
  final FakeWs second;

  StatusProbe get status => container.read(connectionStatusProvider.notifier) as StatusProbe;
  InboxProbe get inbox => container.read(inboxProvider.notifier) as InboxProbe;
}

Future<Rig> rig(WidgetTester tester) async {
  final FakeWs first = mac();
  final FakeWs second = mac();
  final MemorySecretStore secrets = MemorySecretStore()
    ..values['macs.v1'] = encodeProfiles(<MacProfile>[testMac, otherMac])
    ..values['macs.active'] = testMac.id;
  final ProviderContainer container = ProviderContainer(overrides: <Override>[
    secretStoreProvider.overrideWithValue(secrets),
    chatCacheProvider.overrideWithValue(MemoryChatCache()),
    archiveStoreProvider.overrideWithValue(MemoryArchiveStore()),
    wsConnectorProvider.overrideWithValue((Endpoint endpoint) async => endpoint.host == '10.0.0.2' ? second : first),
    networkChangesProvider.overrideWithValue(const Stream<void>.empty()),
    connectionStatusProvider.overrideWith(StatusProbe.new),
    inboxProvider.overrideWith(InboxProbe.new),
  ]);
  addTearDown(container.dispose);
  await tester.runAsync(() => connect(container));
  container.listen<InboxState>(inboxProvider, (InboxState? a, InboxState b) {});
  container.listen<ConnectionStatus>(connectionStatusProvider, (ConnectionStatus? a, ConnectionStatus b) {});
  await tester.pumpWidget(UncontrolledProviderScope(
    container: container,
    child: host(
      Scaffold(
        body: Consumer(
          builder: (BuildContext context, WidgetRef ref, Widget? _) {
            listenMachinesOffers(context, ref);
            return const SizedBox();
          },
        ),
      ),
      screen: true,
    ),
  ));
  return Rig(container, first, second);
}

void press(WidgetTester tester, String label) => tester.widget<TextButton>(find.widgetWithText(TextButton, label)).onPressed!();

Future<void> settleUi(WidgetTester tester) async {
  await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 80)));
  await tester.pump();
  await tester.pump(const Duration(milliseconds: 500));
}

Future<void> dismissSheet(WidgetTester tester) async {
  await tester.tapAt(const Offset(10, 10));
  await tester.pump(const Duration(milliseconds: 500));
  await tester.pump(const Duration(milliseconds: 500));
}

void main() {
  group('the popup buttons', () {
    testWidgets('a double tap on a machine asks once and sends one promote', (WidgetTester tester) async {
      final FakeWs socket = mac();
      await openSheet(tester, socket);
      final InkWell row = tester.widget<InkWell>(find.ancestor(of: find.text('m1'), matching: find.byType(InkWell)));
      row.onTap!();
      row.onTap!();
      await tester.pump(const Duration(milliseconds: 500));
      expect(find.byType(AlertDialog), findsOneWidget);
      await tester.tap(find.text('Move'));
      await settle(tester, socket, promotes: 1);
      expect(promoteRequests(socket), hasLength(1));
    });

    testWidgets('a double tap on Move sends one promote and leaves the sheet open', (WidgetTester tester) async {
      final FakeWs socket = mac();
      await openSheet(tester, socket);
      await tester.tap(find.text('m1'));
      await tester.pump(const Duration(milliseconds: 500));
      press(tester, 'Move');
      press(tester, 'Move');
      await settle(tester, socket, promotes: 1);
      expect(promoteRequests(socket), hasLength(1));
      expect(find.text('Choose a Machine'), findsOneWidget);
    });

    testWidgets('a double tap on Move Anyway sends one confirmed promote and leaves the sheet open', (WidgetTester tester) async {
      final FakeWs socket = mac(promotes: <Map<String, Object?>>[
        <String, Object?>{'needsConfirm': 'm5 can not be reached.'},
        <String, Object?>{'step': 'bump'},
      ]);
      await openSheet(tester, socket);
      await tester.tap(find.text('m1'));
      await tester.pump(const Duration(milliseconds: 500));
      await tester.tap(find.text('Move'));
      await settle(tester, socket, promotes: 1);
      press(tester, 'Move Anyway');
      press(tester, 'Move Anyway');
      await settle(tester, socket, promotes: 2);
      expect(promoteRequests(socket), hasLength(2));
      expect(promoteRequests(socket).last, containsPair('confirmed', true));
      expect(find.text('Choose a Machine'), findsOneWidget);
    });

    testWidgets('Cancel on the Move Anyway question sends nothing more', (WidgetTester tester) async {
      final FakeWs socket = mac(promotes: <Map<String, Object?>>[
        <String, Object?>{'needsConfirm': 'm5 can not be reached.'},
      ]);
      await openSheet(tester, socket);
      await tester.tap(find.text('m1'));
      await tester.pump(const Duration(milliseconds: 500));
      await tester.tap(find.text('Move'));
      await settle(tester, socket, promotes: 1);
      await tester.tap(find.text('Cancel'));
      await tester.pumpAndSettle();
      expect(find.byType(AlertDialog), findsNothing);
      expect(promoteRequests(socket), hasLength(1));
      expect(find.text('Choose a Machine'), findsOneWidget);
    });

    testWidgets('a machine that can be master but is not ready says so and cannot be picked', (WidgetTester tester) async {
      final FakeWs socket = mac(machines: <Map<String, Object?>>[machine('7777777777777777', 'm9', ready: false)]);
      await openSheet(tester, socket);
      expect(find.text('Not ready to take over'), findsOneWidget);
      await tester.tap(find.text('m9'));
      await tester.pump(const Duration(milliseconds: 500));
      expect(find.byType(AlertDialog), findsNothing);
      expect(promoteRequests(socket), isEmpty);
    });

    testWidgets('a sheet opened for one computer sends nothing once another is active', (WidgetTester tester) async {
      final FakeWs socket = mac();
      final FakeWs other = mac();
      final MemorySecretStore secrets = MemorySecretStore()
        ..values['macs.v1'] = encodeProfiles(<MacProfile>[testMac, otherMac])
        ..values['macs.active'] = testMac.id;
      final ProviderContainer container = ProviderContainer(overrides: <Override>[
        secretStoreProvider.overrideWithValue(secrets),
        chatCacheProvider.overrideWithValue(MemoryChatCache()),
        archiveStoreProvider.overrideWithValue(MemoryArchiveStore()),
        wsConnectorProvider.overrideWithValue((Endpoint endpoint) async => endpoint.host == '10.0.0.2' ? other : socket),
        networkChangesProvider.overrideWithValue(const Stream<void>.empty()),
        connectionStatusProvider.overrideWith(StatusProbe.new),
      ]);
      addTearDown(container.dispose);
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
      await tester.runAsync(() => until(() => machineRequests(socket) == 1));
      await settleUi(tester);
      expect(find.text('m1'), findsOneWidget);
      await tester.runAsync(() => container.read(macsProvider.notifier).setActive('n'));
      await tester.pump();
      await tester.tap(find.text('m1'));
      await tester.pump(const Duration(milliseconds: 500));
      await tester.tap(find.text('Move'));
      await settleUi(tester);
      expect(promoteRequests(socket), isEmpty);
      expect(promoteRequests(other), isEmpty);
      expect(find.text('Choose a Machine'), findsNothing);
    });
  });

  group('the offer', () {
    testWidgets('there is none once the machine the phone reached is the primary', (WidgetTester tester) async {
      final Rig r = await rig(tester);
      expect(r.container.read(machinesOfferProvider), isNull);
      r.status.put(reachedPrimary);
      expect(r.container.read(machinesOfferProvider), isNull);
      r.status.put(fallback);
      expect(r.container.read(machinesOfferProvider), const MachinesOffer('m'));
      await settleUi(tester);
      await dismissSheet(tester);
    });

    testWidgets('an outage opens one sheet however its offer changes', (WidgetTester tester) async {
      final Rig r = await rig(tester);
      r.status.put(fallback);
      await settleUi(tester);
      expect(find.text('Choose a Machine'), findsOneWidget);
      expect(machineRequests(r.first), 1);

      r.inbox.put(<InboxItem>[outage('primary-unreachable:3', 'primary_unreachable')]);
      await settleUi(tester);
      r.inbox.put(<InboxItem>[outage('primary-unreachable:3', 'primary_unreachable'), outage('primary-unreachable:3:confirm', 'primary_unreachable')]);
      await settleUi(tester);
      r.status.put(plain);
      await settleUi(tester);
      r.status.put(fallback);
      await settleUi(tester);
      expect(find.text('Choose a Machine'), findsOneWidget);
      expect(machineRequests(r.first), 1);
    });

    testWidgets('a dismissed popup stays closed for the same outage and returns for a new one', (WidgetTester tester) async {
      final Rig r = await rig(tester);
      r.status.put(fallback);
      await settleUi(tester);
      await dismissSheet(tester);
      expect(find.text('Choose a Machine'), findsNothing);

      r.inbox.put(<InboxItem>[outage('primary-unreachable:3', 'primary_unreachable')]);
      await settleUi(tester);
      r.inbox.put(<InboxItem>[outage('primary-unreachable:3', 'primary_unreachable'), outage('primary-unreachable:3:confirm', 'primary_unreachable')]);
      await settleUi(tester);
      r.status.put(plain);
      await settleUi(tester);
      expect(find.text('Choose a Machine'), findsNothing);
      expect(machineRequests(r.first), 1);

      r.inbox.put(const <InboxItem>[]);
      await settleUi(tester);
      r.status.put(fallback);
      await settleUi(tester);
      expect(find.text('Choose a Machine'), findsOneWidget);
      expect(machineRequests(r.first), 2);
    });

    testWidgets('an offer that came while the app was in the background shows on resume', (WidgetTester tester) async {
      final Rig r = await rig(tester);
      r.container.read(lifecycleProvider.notifier).set(AppLifecycleState.paused);
      r.status.put(fallback);
      await settleUi(tester);
      expect(find.text('Choose a Machine'), findsNothing);
      expect(machineRequests(r.first), 0);
      r.container.read(lifecycleProvider.notifier).set(AppLifecycleState.resumed);
      await settleUi(tester);
      expect(find.text('Choose a Machine'), findsOneWidget);
      expect(machineRequests(r.first), 1);
    });

    testWidgets('each computer has its own offer and its sheet asks that computer', (WidgetTester tester) async {
      final Rig r = await rig(tester);
      r.status.put(fallback);
      await settleUi(tester);
      expect(machineRequests(r.first), 1);
      await dismissSheet(tester);

      r.status.put(const ConnectionStatus(LinkPhase.connecting));
      await tester.runAsync(() => r.container.read(macsProvider.notifier).setActive('n'));
      await tester.runAsync(() => until(() => r.container.read(bridgeClientProvider)?.isConnected ?? false));
      r.status.put(fallback);
      await tester.pump();
      await tester.runAsync(() => until(() => machineRequests(r.second) == 1));
      await settleUi(tester);
      expect(find.text('Choose a Machine'), findsOneWidget);
      expect(machineRequests(r.first), 1);
      await dismissSheet(tester);

      r.status.put(const ConnectionStatus(LinkPhase.connecting));
      await tester.runAsync(() => r.container.read(macsProvider.notifier).setActive('m'));
      await tester.runAsync(() => until(() => r.container.read(bridgeClientProvider)?.isConnected ?? false));
      r.status.put(fallback);
      await settleUi(tester);
      expect(find.text('Choose a Machine'), findsNothing);
      expect(machineRequests(r.first), 1);
      expect(machineRequests(r.second), 1);
    });
  });
}
