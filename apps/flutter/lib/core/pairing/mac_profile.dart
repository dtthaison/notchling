import 'dart:convert';

import '../net/tunnel.dart';
import '../net/ws_transport.dart';
import '../protocol/json.dart';

class PinnedNode {
  const PinnedNode({
    required this.id,
    required this.name,
    required this.addrs,
    required this.fingerprint,
    this.canBeMaster = true,
    this.online = true,
  });

  factory PinnedNode.fromJson(Json json) => PinnedNode(
        id: json.str('id'),
        name: json.str('name'),
        addrs: json.strings('addrs'),
        fingerprint: json.str('fp'),
        canBeMaster: json.boolean('canBeMaster', orElse: true),
        online: json.boolean('online', orElse: true),
      );

  final String id;
  final String name;
  final List<String> addrs;
  final String fingerprint;
  final bool canBeMaster;
  final bool online;

  bool get hasValidPin => RegExp(r'^[0-9a-f]{64}$').hasMatch(fingerprint.trim().toLowerCase());

  Json toJson() => <String, Object?>{
        'id': id,
        'name': name,
        'addrs': addrs,
        'fp': fingerprint,
        'canBeMaster': canBeMaster,
        'online': online,
      };
}

class MacProfile {
  const MacProfile({
    required this.id,
    required this.name,
    required this.hosts,
    required this.port,
    required this.fingerprint,
    required this.token,
    this.lastGoodHost,
    this.tunnel,
    this.lastGoodWasTunnel = false,
    this.nodes = const <PinnedNode>[],
    this.clusterPrimary,
  });

  factory MacProfile.fromJson(Json json) => MacProfile(
        id: json.str('id'),
        name: json.str('name'),
        hosts: json.strings('hosts'),
        port: json.integer('port'),
        fingerprint: json.str('fp'),
        token: json.str('token'),
        lastGoodHost: json.strOrNull('lastGoodHost'),
        tunnel: json['tunnel'] is Map<Object?, Object?> ? TunnelInfo.fromJson(json.object('tunnel')) : null,
        lastGoodWasTunnel: json['lastGoodWasTunnel'] == true,
        nodes: _readNodes(json['nodes']),
        clusterPrimary: json.strOrNull('clusterPrimary'),
      );

  static List<PinnedNode> _readNodes(Object? raw) {
    if (raw is! List<Object?>) return const <PinnedNode>[];
    final List<PinnedNode> nodes = <PinnedNode>[];
    for (final Object? item in raw) {
      if (item is! Map<Object?, Object?>) continue;
      try {
        nodes.add(PinnedNode.fromJson(item.cast<String, Object?>()));
      } on FormatException catch (_) {
      }
    }
    return nodes;
  }

  final String id;
  final String name;
  final List<String> hosts;
  final int port;
  final String fingerprint;
  final String token;
  final String? lastGoodHost;

  final TunnelInfo? tunnel;
  final bool lastGoodWasTunnel;

  final List<PinnedNode> nodes;
  final String? clusterPrimary;

  List<String> get orderedHosts {
    final String? good = lastGoodHost;
    if (good == null || !hosts.contains(good)) return hosts;
    return <String>[good, ...hosts.where((String h) => h != good)];
  }

  List<Endpoint> endpoints({bool preferLan = false}) {
    final TunnelInfo? via = tunnel;
    final Duration lanTimeout = via == null ? const Duration(seconds: 6) : const Duration(seconds: 3);
    Endpoint lan(String host) => Endpoint(host: host, port: port, fingerprint: fingerprint, timeout: lanTimeout);
    final List<Endpoint> routes = <Endpoint>[
      for (final String host in orderedHosts) lan(host),
      if (via != null) Endpoint.viaTunnel(via),
    ];
    if (via != null && lastGoodWasTunnel && !preferLan) {
      return <Endpoint>[routes.last, ...routes.take(routes.length - 1)];
    }
    return routes;
  }

  MacProfile withLastGoodHost(String host) => MacProfile(
      id: id, name: name, hosts: hosts, port: port, fingerprint: fingerprint, token: token, lastGoodHost: host, tunnel: tunnel,
      nodes: nodes, clusterPrimary: clusterPrimary);

  MacProfile withLastGoodTunnel() => MacProfile(
      id: id, name: name, hosts: hosts, port: port, fingerprint: fingerprint, token: token, lastGoodHost: lastGoodHost, tunnel: tunnel,
      lastGoodWasTunnel: true, nodes: nodes, clusterPrimary: clusterPrimary);

  MacProfile withCluster(String? primary, List<PinnedNode> learned) => MacProfile(
      id: id, name: name, hosts: hosts, port: port, fingerprint: fingerprint, token: token, lastGoodHost: lastGoodHost, tunnel: tunnel,
      lastGoodWasTunnel: lastGoodWasTunnel, nodes: learned, clusterPrimary: primary);

  Json toJson() => <String, Object?>{
        'id': id,
        'name': name,
        'hosts': hosts,
        'port': port,
        'fp': fingerprint,
        'token': token,
        'lastGoodHost': lastGoodHost,
        if (tunnel != null) 'tunnel': tunnel!.toJson(),
        'lastGoodWasTunnel': lastGoodWasTunnel,
        if (nodes.isNotEmpty) 'nodes': <Object?>[for (final PinnedNode n in nodes) n.toJson()],
        'clusterPrimary': ?clusterPrimary,
      };
}

String encodeProfiles(List<MacProfile> profiles) => jsonEncode(profiles.map((MacProfile p) => p.toJson()).toList());

List<MacProfile> decodeProfiles(String text) {
  final Object? decoded = jsonDecode(text);
  if (decoded is! List<Object?>) throw const FormatException('profiles are not a list');
  return decoded.map((Object? item) => MacProfile.fromJson((item as Map<Object?, Object?>).cast<String, Object?>())).toList();
}
