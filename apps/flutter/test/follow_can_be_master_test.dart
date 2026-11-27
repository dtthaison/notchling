
import 'package:agent_console/core/net/bridge_client.dart';
import 'package:agent_console/app/providers.dart';
import 'package:agent_console/core/pairing/mac_profile.dart';
import 'package:agent_console/core/pairing/mac_store.dart';
import 'package:agent_console/features/macs/macs_notifier.dart';
import 'package:agent_console/features/connection/connection.dart';
import 'package:agent_console/features/sessions/held_notifier.dart';
import 'package:agent_console/features/sessions/held_section.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/cluster_world.dart';
import 'support/fake_ws.dart';
import 'support/harness.dart';

void main() {
  test('falls_back_to_a_reachable_can_be_master_machine', () async {
    final ClusterWorld world = ClusterWorld();
    String? learnedPrimary;
    List<PinnedNode> learned = const <PinnedNode>[];
    final BridgeClient client = clusterClient(world, onCluster: (String? primary, int term, List<PinnedNode> nodes) {
      learnedPrimary = primary;
      learned = nodes;
    });
    client.start();
    await until(() => client.isConnected);

    expect(client.currentStatus.via, isNull);
    expect(learnedPrimary, 'm5');
    expect(learned.map((PinnedNode n) => n.name), <String>['m5', 'm1']);
    expect(learned.last.fingerprint, fpM1);
    expect(client.inCluster, isTrue);

    world.dialed.clear();
    await world.primaryGoesDown();
    await viaM1(client);

    expect(world.dialed, <String>['10.0.0.5:47821', '10.0.0.6:47821'], reason: 'the primary first, then m1; m5 is not tried twice');
    expect(client.currentStatus.primaryName, 'm5');
    expect(client.nodeName, 'm1');
    expect(world.m1.single.sent.first['op'], 'hello');
    expect(world.m1.single.sent.first['token'], 'tok');
    await client.dispose();

    final ClusterWorld again = ClusterWorld();
    final ProviderContainer container = clusterContainer(again);
    await connectContainer(container);
    final MacProfile saved = container.read(macsProvider).byId('m5')!;
    expect(saved.nodes.map((PinnedNode n) => n.name), <String>['m5', 'm1']);
    expect(saved.nodes.last.fingerprint, fpM1);
    expect(saved.clusterPrimary, 'm5');
    final MemorySecretStore store = container.read(secretStoreProvider) as MemorySecretStore;
    await until(() => store.values['macs.v1']!.contains(fpM1));
    expect(decodeProfiles(store.values['macs.v1']!).single.nodes.last.fingerprint, fpM1);
  });

  test('refuses_a_machine_whose_cert_doesnt_match', () async {
    final ClusterWorld world = ClusterWorld(extraNodes: <Map<String, Object?>>[clusterNode('m2', 'm2', '10.0.0.7:47821', '')]);
    final BridgeClient client = clusterClient(world);
    client.start();
    await until(() => client.isConnected);

    world.m1Cert = 'c' * 64;
    world.dialed.clear();
    await world.primaryGoesDown();
    await until(() => world.dialed.contains('10.0.0.6:47821') && client.currentStatus.refused.isNotEmpty);

    expect(client.isConnected, isFalse);
    expect(client.currentStatus.refused, containsAll(<String>['m1', 'm2']));
    expect(world.m1, isEmpty, reason: 'no connection to m1 was ever made, so its token was never sent');
    expect(world.dialed, isNot(contains('10.0.0.7:47821')), reason: 'a machine with no fingerprint is never dialed');
    expect(world.m1Requests, isEmpty);
    await client.dispose();
  });

  test('returns_to_primary_when_it_answers', () async {
    final ClusterWorld world = ClusterWorld();
    final BridgeClient client = clusterClient(world, primaryRetry: const Duration(milliseconds: 30));
    client.start();
    await until(() => client.isConnected);
    await client.subscribe(const SubscriptionSpec(sessions: true));
    await world.primaryGoesDown();
    await viaM1(client);
    client.markHeld(true);
    expect(client.currentStatus.primaryAnswers, isFalse);

    world.m5Up = true;
    await until(() => client.currentStatus.primaryAnswers);
    expect(client.currentStatus.via, 'm1', reason: 'it stays while the caller says it should');
    expect(client.isConnected, isTrue);

    client.markHeld(false);
    await until(() => client.isConnected && client.currentStatus.via == null && world.m5.length >= 2);
    expect(client.currentStatus.primaryAnswers, isFalse);
    expect(world.m1.last.closed, isTrue);
    final FakeWs back = world.m5.last;
    expect(back.sent.first['op'], 'hello');
    await until(() => back.sent.any((Map<String, Object?> m) => m['op'] == 'subscribe'));
    await client.dispose();
  });

  testWidgets('held_command_shows_and_cancels', (WidgetTester tester) async {
    final ClusterWorld world = ClusterWorld()..m5Up = false;
    late ProviderContainer container;
    await tester.runAsync(() async {
      container = clusterContainerWith(world, pinnedM5());
      await connectContainer(container);
      await until(() => container.read(bridgeClientProvider)?.currentStatus.via == 'm1');
      container.listen<HeldState>(heldProvider, (HeldState? a, HeldState b) {});
      await container.read(bridgeClientProvider)!.sessionMessage('s1', 'Run the tests again');
      await until(() => container.read(heldProvider).entries.isNotEmpty);
    });
    await tester.pumpWidget(UncontrolledProviderScope(
      container: container,
      child: host(const HeldSection()),
    ));
    expect(find.text('Held on m1'), findsOneWidget);
    expect(find.text('Run the tests again'), findsOneWidget);
    expect(find.text('Cancel'), findsOneWidget);
    expect(find.text('Waiting'), findsNothing);

    await tester.tap(find.text('Cancel'));
    await tester.runAsync(() => until(() => container.read(heldProvider).entries.isEmpty));
    await tester.pump();
    expect(world.m1Requests.where((Map<String, Object?> m) => m['op'] == 'held.cancel').single['heldId'], 'held-1');
    expect(find.text('Held on m1'), findsNothing);
    await tester.pumpWidget(const SizedBox());
    await tester.runAsync(() async => container.dispose());
  });

  testWidgets('stale_held_command_asks_still_send', (WidgetTester tester) async {
    final ClusterWorld world = ClusterWorld()
      ..m5Up = false
      ..held.add(heldEntry('held-9', 'Deploy it', waiting: true));
    late ProviderContainer container;
    await tester.runAsync(() async {
      container = clusterContainerWith(world, pinnedM5(), primaryRetry: const Duration(milliseconds: 30));
      await connectContainer(container);
      await until(() => container.read(bridgeClientProvider)?.currentStatus.via == 'm1');
      container.listen<HeldState>(heldProvider, (HeldState? a, HeldState b) {});
      await until(() => container.read(heldProvider).entries.isNotEmpty);
    });
    await tester.pumpWidget(UncontrolledProviderScope(
      container: container,
      child: host(const HeldSection()),
    ));
    expect(find.text('Waiting'), findsOneWidget);
    expect(find.text('Cancel'), findsOneWidget);
    expect(find.text('Still send?'), findsNothing);
    expect(find.text('Send'), findsNothing);

    await tester.runAsync(() async {
      world.m5Up = true;
      await until(() => container.read(connectionStatusProvider).primaryAnswers);
    });
    await tester.pump();
    expect(find.text('Still send?'), findsOneWidget);
    expect(find.text('Send'), findsOneWidget);
    expect(find.text('Cancel'), findsOneWidget);
    expect(container.read(connectionStatusProvider).via, 'm1', reason: 'the entry lives on m1, so the phone stays there until it is answered');

    await tester.tap(find.text('Send'));
    await tester.runAsync(() => until(() => container.read(heldProvider).entries.isEmpty));
    await tester.pump();
    expect(world.m1Requests.where((Map<String, Object?> m) => m['op'] == 'held.confirm').single['heldId'], 'held-9');
    expect(find.text('Still send?'), findsNothing);

    await tester.runAsync(() => until(() => container.read(connectionStatusProvider).via == null && container.read(connectionStatusProvider).phase == LinkPhase.connected));
    await tester.pumpWidget(const SizedBox());
    await tester.runAsync(() async => container.dispose());
  });

  test('old_mac_without_cluster_field_keeps_one_machine', () async {
    final ClusterWorld world = ClusterWorld(clusterField: false);
    final List<List<PinnedNode>> calls = <List<PinnedNode>>[];
    final BridgeClient client = clusterClient(world, profile: pinnedM5(), onCluster: (String? primary, int term, List<PinnedNode> nodes) => calls.add(nodes));
    client.start();
    await until(() => client.isConnected);

    expect(client.inCluster, isFalse);
    expect(calls.last, isEmpty, reason: 'a Mac that no longer sends cluster leaves the learned pins empty');
    expect(client.currentStatus.via, isNull);

    world.dialed.clear();
    await world.primaryGoesDown();
    await until(() => !client.isConnected && world.dialed.isNotEmpty);
    await Future<void>.delayed(const Duration(milliseconds: 100));
    expect(world.dialed.toSet(), <String>{'10.0.0.5:47821'}, reason: 'only the paired Mac is ever tried');
    expect(client.currentStatus.refused, isEmpty);
    await client.dispose();

    final MacProfile old = MacProfile.fromJson(<String, Object?>{
      'id': 'm',
      'name': 'Mac',
      'hosts': <String>['10.0.0.1'],
      'port': 47821,
      'fp': fpM5,
      'token': 'tok',
    });
    expect(old.nodes, isEmpty);
    expect(old.clusterPrimary, isNull);
    expect(MacProfile.fromJson(pinnedM5().toJson()).nodes.last.fingerprint, fpM1);
  });
}

