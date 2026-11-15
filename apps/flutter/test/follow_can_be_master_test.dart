import 'dart:async';
import 'dart:io';

import 'package:agent_console/app/providers.dart';
import 'package:agent_console/core/net/bridge_client.dart';
import 'package:agent_console/core/net/ws_transport.dart';
import 'package:agent_console/core/pairing/mac_profile.dart';
import 'package:agent_console/core/pairing/mac_store.dart';
import 'package:agent_console/features/chat/chat_cache.dart';
import 'package:agent_console/features/connection/connection.dart';
import 'package:agent_console/features/macs/macs_notifier.dart';
import 'package:agent_console/features/sessions/archive_store.dart';
import 'package:agent_console/features/sessions/held_notifier.dart';
import 'package:agent_console/features/sessions/held_section.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:flutter_test/flutter_test.dart';

import 'support/fake_ws.dart';
import 'support/harness.dart';

const String _fpM5 = '20d3c003d553a68bd969f5663f4b747be987a2874fe13bb025fe07710b829445';
final String _fpM1 = 'b' * 64;

const MacProfile _m5 = MacProfile(
  id: 'm5',
  name: 'm5',
  hosts: <String>['10.0.0.5'],
  port: 47821,
  fingerprint: _fpM5,
  token: 'tok',
);

Map<String, Object?> _node(String id, String name, String addr, String fingerprint, {bool online = true}) => <String, Object?>{
      'id': id,
      'name': name,
      'addrs': <String>[addr],
      'fingerprint': fingerprint,
      'canBeMaster': true,
      'online': online,
    };

Map<String, Object?> _entry(String id, String summary, {bool waiting = false}) => <String, Object?>{
      'id': id,
      'op': 'session.message',
      'sessionId': 's1',
      'summary': summary,
      'at': 1793541465000,
      'waiting': waiting,
    };

class _World {
  _World({this.clusterField = true, List<Map<String, Object?>>? extraNodes}) : extra = extraNodes ?? <Map<String, Object?>>[];

  bool m5Up = true;
  bool clusterField;
  String m1Cert = _fpM1;
  final List<Map<String, Object?>> extra;
  final List<String> dialed = <String>[];
  final List<FakeWs> m5 = <FakeWs>[];
  final List<FakeWs> m1 = <FakeWs>[];
  final List<Map<String, Object?>> held = <Map<String, Object?>>[];
  final List<Map<String, Object?>> m1Requests = <Map<String, Object?>>[];

  Map<String, Object?> hello() => <String, Object?>{
        'version': 1,
        'device': <String, Object?>{'id': 'd', 'name': 'phone'},
        if (clusterField)
          'cluster': <String, Object?>{
            'primary': 'm5',
            'term': 3,
            'nodes': <Object?>[
              _node('m5', 'm5', '10.0.0.5:47821', _fpM5),
              _node('m1', 'm1', '10.0.0.6:47821', _fpM1),
              ...extra,
            ],
          },
      };

  FakeWs? get liveM1 => m1.isEmpty ? null : m1.last;

  void pushHeld() => liveM1?.event('held.changed', <String, Object?>{'entries': List<Object?>.of(held), 'notices': <Object?>[]});

  FakeWs _socket(List<FakeWs> into, {required bool standby}) {
    final FakeWs socket = FakeWs(onSend: (FakeWs s, Map<String, Object?> m) {
      if (standby) m1Requests.add(m);
      final Object id = m['id']!;
      switch (m['op']) {
        case 'hello':
          s.reply(id, data: hello());
        case 'subscribe':
          s.reply(id, data: <String, Object?>{'subscribed': true});
        case 'held.list':
          s.reply(id, data: <String, Object?>{'entries': List<Object?>.of(held), 'notices': <Object?>[]});
        case 'held.cancel':
          held.removeWhere((Map<String, Object?> e) => e['id'] == m['heldId']);
          s.reply(id, data: <String, Object?>{'cancelled': true});
          pushHeld();
        case 'held.confirm':
          held.removeWhere((Map<String, Object?> e) => e['id'] == m['heldId']);
          s.reply(id, data: <String, Object?>{'confirmed': true});
          pushHeld();
        case 'session.message':
          held.add(_entry('held-1', '${m['text']}'));
          s.reply(id, data: <String, Object?>{'held': <String, Object?>{'id': 'held-1', 'at': 1793541465000}});
          pushHeld();
      }
    });
    into.add(socket);
    return socket;
  }

  Future<WsConnection> connect(Endpoint e) async {
    dialed.add('${e.host}:${e.port}');
    switch (e.host) {
      case '10.0.0.5':
        if (!m5Up) throw const SocketException('down');
        if (e.fingerprint != _fpM5) throw const FingerprintMismatch();
        return _socket(m5, standby: false);
      case '10.0.0.6':
        if (e.fingerprint != m1Cert) throw const FingerprintMismatch();
        return _socket(m1, standby: true);
    }
    throw SocketException('no route to ${e.host}');
  }

  Future<void> primaryGoesDown() async {
    m5Up = false;
    await m5.last.drop();
  }
}

BridgeClient _client(
  _World world, {
  MacProfile profile = _m5,
  Duration primaryRetry = const Duration(hours: 1),
  void Function(String?, List<PinnedNode>)? onCluster,
  bool Function()? canReturn,
}) =>
    BridgeClient(
      connector: world.connect,
      profile: profile,
      requestTimeout: const Duration(seconds: 2),
      backoff: const <Duration>[Duration(milliseconds: 20), Duration(milliseconds: 40)],
      primaryRetry: primaryRetry,
      onCluster: onCluster,
      canReturnToPrimary: canReturn,
    );

Future<void> _viaM1(BridgeClient client) => until(() => client.isConnected && client.currentStatus.via == 'm1');

ProviderContainer _container(_World world, {MacProfile profile = _m5, Duration primaryRetry = const Duration(hours: 1)}) {
  final MemorySecretStore secrets = MemorySecretStore()
    ..values['macs.v1'] = encodeProfiles(<MacProfile>[profile])
    ..values['macs.active'] = profile.id;
  final ProviderContainer fresh = ProviderContainer(overrides: <Override>[
    secretStoreProvider.overrideWithValue(secrets),
    chatCacheProvider.overrideWithValue(MemoryChatCache()),
    archiveStoreProvider.overrideWithValue(MemoryArchiveStore()),
    wsConnectorProvider.overrideWithValue(world.connect),
    networkChangesProvider.overrideWithValue(const Stream<void>.empty()),
    primaryRetryProvider.overrideWithValue(primaryRetry),
  ]);
  addTearDown(fresh.dispose);
  return fresh;
}

MacProfile _withPinnedM1() => _m5.withCluster('m5', <PinnedNode>[
      PinnedNode(id: 'm5', name: 'm5', addrs: const <String>['10.0.0.5:47821'], fingerprint: _fpM5),
      PinnedNode(id: 'm1', name: 'm1', addrs: const <String>['10.0.0.6:47821'], fingerprint: _fpM1),
    ]);

void main() {
  test('falls_back_to_a_reachable_can_be_master_machine', () async {
    final _World world = _World();
    String? learnedPrimary;
    List<PinnedNode> learned = const <PinnedNode>[];
    final BridgeClient client = _client(world, onCluster: (String? primary, List<PinnedNode> nodes) {
      learnedPrimary = primary;
      learned = nodes;
    });
    client.start();
    await until(() => client.isConnected);

    expect(client.currentStatus.via, isNull);
    expect(learnedPrimary, 'm5');
    expect(learned.map((PinnedNode n) => n.name), <String>['m5', 'm1']);
    expect(learned.last.fingerprint, _fpM1);
    expect(client.inCluster, isTrue);

    world.dialed.clear();
    await world.primaryGoesDown();
    await _viaM1(client);

    expect(world.dialed, <String>['10.0.0.5:47821', '10.0.0.6:47821'], reason: 'the primary first, then m1; m5 is not tried twice');
    expect(client.currentStatus.primaryName, 'm5');
    expect(client.nodeName, 'm1');
    expect(world.m1.single.sent.first['op'], 'hello');
    expect(world.m1.single.sent.first['token'], 'tok');
    await client.dispose();

    final _World again = _World();
    final ProviderContainer container = _container(again);
    await connectContainer(container);
    final MacProfile saved = container.read(macsProvider).byId('m5')!;
    expect(saved.nodes.map((PinnedNode n) => n.name), <String>['m5', 'm1']);
    expect(saved.nodes.last.fingerprint, _fpM1);
    expect(saved.clusterPrimary, 'm5');
    final MemorySecretStore store = container.read(secretStoreProvider) as MemorySecretStore;
    await until(() => store.values['macs.v1']!.contains(_fpM1));
    expect(decodeProfiles(store.values['macs.v1']!).single.nodes.last.fingerprint, _fpM1);
  });

  test('refuses_a_machine_whose_cert_doesnt_match', () async {
    final _World world = _World(extraNodes: <Map<String, Object?>>[_node('m2', 'm2', '10.0.0.7:47821', '')]);
    final BridgeClient client = _client(world);
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
    final _World world = _World();
    bool allowReturn = false;
    final BridgeClient client = _client(world, primaryRetry: const Duration(milliseconds: 30), canReturn: () => allowReturn);
    client.start();
    await until(() => client.isConnected);
    await client.subscribe(const SubscriptionSpec(sessions: true));
    await world.primaryGoesDown();
    await _viaM1(client);
    expect(client.currentStatus.primaryAnswers, isFalse);

    world.m5Up = true;
    await until(() => client.currentStatus.primaryAnswers);
    expect(client.currentStatus.via, 'm1', reason: 'it stays while the caller says it should');
    expect(client.isConnected, isTrue);

    allowReturn = true;
    await until(() => client.isConnected && client.currentStatus.via == null && world.m5.length >= 2);
    expect(client.currentStatus.primaryAnswers, isFalse);
    expect(world.m1.last.closed, isTrue);
    final FakeWs back = world.m5.last;
    expect(back.sent.first['op'], 'hello');
    await until(() => back.sent.any((Map<String, Object?> m) => m['op'] == 'subscribe'));
    await client.dispose();
  });

  testWidgets('held_command_shows_and_cancels', (WidgetTester tester) async {
    final _World world = _World()..m5Up = false;
    late ProviderContainer container;
    await tester.runAsync(() async {
      container = _containerWithProfile(world, _withPinnedM1());
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
    final _World world = _World()
      ..m5Up = false
      ..held.add(_entry('held-9', 'Deploy it', waiting: true));
    late ProviderContainer container;
    await tester.runAsync(() async {
      container = _containerWithProfile(world, _withPinnedM1(), primaryRetry: const Duration(milliseconds: 30));
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
    final _World world = _World(clusterField: false);
    final List<List<PinnedNode>> calls = <List<PinnedNode>>[];
    final BridgeClient client = _client(world, profile: _withPinnedM1(), onCluster: (String? primary, List<PinnedNode> nodes) => calls.add(nodes));
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
      'fp': _fpM5,
      'token': 'tok',
    });
    expect(old.nodes, isEmpty);
    expect(old.clusterPrimary, isNull);
    expect(MacProfile.fromJson(_withPinnedM1().toJson()).nodes.last.fingerprint, _fpM1);
  });
}

Future<void> connectContainer(ProviderContainer container) async {
  container.read(macsProvider);
  await until(() => container.read(macsProvider).loaded);
  container.listen<int>(connectedCountProvider, (int? a, int b) {});
  container.listen<ConnectionStatus>(connectionStatusProvider, (ConnectionStatus? a, ConnectionStatus b) {});
  await until(() => container.read(bridgeClientProvider)?.isConnected ?? false);
}

ProviderContainer _containerWithProfile(_World world, MacProfile profile, {Duration primaryRetry = const Duration(hours: 1)}) =>
    _container(world, profile: profile, primaryRetry: primaryRetry);
