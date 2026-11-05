import 'dart:convert';

import '../net/tunnel.dart';
import '../net/ws_transport.dart';
import '../protocol/json.dart';

/// A paired Mac: how to reach it, the certificate fingerprint it is pinned to, and our device token.
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
      );

  final String id;
  final String name;
  final List<String> hosts;
  final int port;
  final String fingerprint;
  final String token;
  final String? lastGoodHost;

  /// The route through the Cloudflare tunnel, when the QR carried one.
  final TunnelInfo? tunnel;
  final bool lastGoodWasTunnel;

  /// The host that worked last time goes first.
  List<String> get orderedHosts {
    final String? good = lastGoodHost;
    if (good == null || !hosts.contains(good)) return hosts;
    return <String>[good, ...hosts.where((String h) => h != good)];
  }

  /// The routes in the order to try them: the one that worked last, then the LAN hosts, then the tunnel.
  /// With [preferLan] (the network just changed) the LAN goes first even if the tunnel worked last, so
  /// arriving home moves the link off the internet.
  List<Endpoint> endpoints({bool preferLan = false}) {
    final TunnelInfo? via = tunnel;
    // With a tunnel to fall back on, an unreachable LAN address should not hold things up for long.
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
      id: id, name: name, hosts: hosts, port: port, fingerprint: fingerprint, token: token, lastGoodHost: host, tunnel: tunnel);

  MacProfile withLastGoodTunnel() => MacProfile(
      id: id, name: name, hosts: hosts, port: port, fingerprint: fingerprint, token: token, lastGoodHost: lastGoodHost, tunnel: tunnel,
      lastGoodWasTunnel: true);

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
      };
}

String encodeProfiles(List<MacProfile> profiles) => jsonEncode(profiles.map((MacProfile p) => p.toJson()).toList());

List<MacProfile> decodeProfiles(String text) {
  final Object? decoded = jsonDecode(text);
  if (decoded is! List<Object?>) throw const FormatException('profiles are not a list');
  return decoded.map((Object? item) => MacProfile.fromJson((item as Map<Object?, Object?>).cast<String, Object?>())).toList();
}
