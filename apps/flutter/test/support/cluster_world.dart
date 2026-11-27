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
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:flutter_test/flutter_test.dart';

import 'fake_ws.dart';
import 'harness.dart';

const String fpM5 = '20d3c003d553a68bd969f5663f4b747be987a2874fe13bb025fe07710b829445';
final String fpM1 = 'b' * 64;

const MacProfile m5Profile = MacProfile(
  id: 'm5',
  name: 'm5',
  hosts: <String>['10.0.0.5'],
  port: 47821,
  fingerprint: fpM5,
  token: 'tok',
);

Map<String, Object?> clusterNode(String id, String name, String addr, String fingerprint, {bool online = true}) => <String, Object?>{
      'id': id,
      'name': name,
      'addrs': <String>[addr],
      'fingerprint': fingerprint,
      'canBeMaster': true,
      'online': online,
    };

Map<String, Object?> heldEntry(String id, String summary, {bool waiting = false}) => <String, Object?>{
      'id': id,
      'op': 'session.message',
      'sessionId': 's1',
      'summary': summary,
      'at': 1793541465000,
      'waiting': waiting,
    };

class ClusterWorld {
  ClusterWorld({this.clusterField = true, List<Map<String, Object?>>? extraNodes}) : extra = extraNodes ?? <Map<String, Object?>>[];

  bool m5Up = true;
  bool clusterField;
  String m1Cert = fpM1;
  final List<Map<String, Object?>> extra;
  final List<String> dialed = <String>[];
  final List<FakeWs> m5 = <FakeWs>[];
  final List<FakeWs> m1 = <FakeWs>[];
  final List<Map<String, Object?>> held = <Map<String, Object?>>[];
  final List<Map<String, Object?>> m1Requests = <Map<String, Object?>>[];

  int term = 3;
  String primaryId = 'm5';
  bool hangPrimary = false;
  String? heldListError;
  Map<String, Object?>? cancelOverride;
  Map<String, Object?>? m1ClusterOverride;

  Map<String, Object?> hello({bool standby = false}) => <String, Object?>{
        'version': 1,
        'device': <String, Object?>{'id': 'd', 'name': 'phone'},
        if (clusterField)
          'cluster': standby && m1ClusterOverride != null
              ? m1ClusterOverride
              : <String, Object?>{
                  'primary': primaryId,
                  'term': term,
                  'nodes': <Object?>[
                    clusterNode('m5', 'm5', '10.0.0.5:47821', fpM5),
                    clusterNode('m1', 'm1', '10.0.0.6:47821', fpM1),
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
          s.reply(id, data: hello(standby: standby));
        case 'subscribe':
          s.reply(id, data: <String, Object?>{'subscribed': true});
        case 'chat.get':
          if (standby) {
            s.fail(id, 'primary_unreachable', 'm5 can not be reached');
          } else {
            s.reply(id, data: <String, Object?>{'messages': <Object?>[], 'hasMore': false});
          }
        case 'held.list':
          if (heldListError != null) {
            s.fail(id, heldListError!);
            break;
          }
          s.reply(id, data: <String, Object?>{'entries': List<Object?>.of(held), 'notices': <Object?>[]});
        case 'held.cancel':
          if (cancelOverride != null) {
            s.reply(id, data: cancelOverride!);
            break;
          }
          held.removeWhere((Map<String, Object?> e) => e['id'] == m['heldId']);
          s.reply(id, data: <String, Object?>{'cancelled': true});
          pushHeld();
        case 'held.confirm':
          held.removeWhere((Map<String, Object?> e) => e['id'] == m['heldId']);
          s.reply(id, data: <String, Object?>{'confirmed': true});
          pushHeld();
        case 'session.message':
          held.add(heldEntry('held-1', '${m['text']}'));
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
        if (hangPrimary) return Completer<WsConnection>().future;
        if (!m5Up) throw const SocketException('down');
        if (e.fingerprint != fpM5) throw const FingerprintMismatch();
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

BridgeClient clusterClient(
  ClusterWorld world, {
  MacProfile profile = m5Profile,
  Duration primaryRetry = const Duration(hours: 1),
  void Function(String?, int, List<PinnedNode>)? onCluster,
  DateTime Function()? clock,
}) =>
    BridgeClient(
      connector: world.connect,
      profile: profile,
      requestTimeout: const Duration(seconds: 2),
      backoff: const <Duration>[Duration(milliseconds: 20), Duration(milliseconds: 40)],
      primaryRetry: primaryRetry,
      onCluster: onCluster,
      clock: clock,
    );

Future<void> viaM1(BridgeClient client) => until(() => client.isConnected && client.currentStatus.via == 'm1');

ProviderContainer clusterContainer(
  ClusterWorld world, {
  MacProfile profile = m5Profile,
  List<MacProfile> others = const <MacProfile>[],
  WsConnector? connector,
  Duration primaryRetry = const Duration(hours: 1),
}) {
  final MemorySecretStore secrets = MemorySecretStore()
    ..values['macs.v1'] = encodeProfiles(<MacProfile>[profile, ...others])
    ..values['macs.active'] = profile.id;
  final ProviderContainer fresh = ProviderContainer(overrides: <Override>[
    secretStoreProvider.overrideWithValue(secrets),
    chatCacheProvider.overrideWithValue(MemoryChatCache()),
    archiveStoreProvider.overrideWithValue(MemoryArchiveStore()),
    wsConnectorProvider.overrideWithValue(connector ?? world.connect),
    networkChangesProvider.overrideWithValue(const Stream<void>.empty()),
    primaryRetryProvider.overrideWithValue(primaryRetry),
  ]);
  addTearDown(fresh.dispose);
  return fresh;
}

MacProfile pinnedM5() => m5Profile.withCluster('m5', <PinnedNode>[
      PinnedNode(id: 'm5', name: 'm5', addrs: const <String>['10.0.0.5:47821'], fingerprint: fpM5),
      PinnedNode(id: 'm1', name: 'm1', addrs: const <String>['10.0.0.6:47821'], fingerprint: fpM1),
    ], term: 3);

Future<void> connectContainer(ProviderContainer container) async {
  container.read(macsProvider);
  await until(() => container.read(macsProvider).loaded);
  container.listen<int>(connectedCountProvider, (int? a, int b) {});
  container.listen<ConnectionStatus>(connectionStatusProvider, (ConnectionStatus? a, ConnectionStatus b) {});
  await until(() => container.read(bridgeClientProvider)?.isConnected ?? false);
}

ProviderContainer clusterContainerWith(
  ClusterWorld world,
  MacProfile profile, {
  List<MacProfile> others = const <MacProfile>[],
  WsConnector? connector,
  Duration primaryRetry = const Duration(hours: 1),
}) =>
    clusterContainer(world, profile: profile, others: others, connector: connector, primaryRetry: primaryRetry);

