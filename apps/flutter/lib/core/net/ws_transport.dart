import 'dart:async';
import 'dart:io';
import 'dart:typed_data';

import 'package:crypto/crypto.dart';

import 'tunnel.dart';

enum RouteKind { lan, tunnel }

class Endpoint {
  const Endpoint({
    required this.host,
    required this.port,
    required this.fingerprint,
    this.tunnel,
    this.timeout = const Duration(seconds: 6),
  });

  Endpoint.viaTunnel(TunnelInfo info, {this.timeout = const Duration(seconds: 10)})
      : host = info.host,
        port = info.uri.hasPort ? info.uri.port : 443,
        fingerprint = '',
        tunnel = info;

  final String host;
  final int port;
  final String fingerprint;
  final TunnelInfo? tunnel;

  final Duration timeout;

  RouteKind get kind => tunnel == null ? RouteKind.lan : RouteKind.tunnel;

  @override
  String toString() => '$host:$port';
}

abstract class WsConnection {
  Stream<Object> get messages;
  void sendText(String text);
  Future<void> close();
}

typedef WsConnector = Future<WsConnection> Function(Endpoint endpoint);

bool certificateMatchesFingerprint(Uint8List der, String fingerprint) {
  final String actual = sha256.convert(der).toString();
  return actual == fingerprint.trim().toLowerCase();
}

class FingerprintMismatch implements Exception {
  const FingerprintMismatch();

  @override
  String toString() => 'FingerprintMismatch';
}

const String bridgeSubprotocol = 'agentconsole.v1';

Future<WsConnection> connectEndpoint(Endpoint endpoint) =>
    endpoint.tunnel == null ? connectPinned(endpoint) : connectTunnel(endpoint);

Future<WsConnection> connectTunnel(Endpoint endpoint, {String scheme = 'wss'}) async {
  final TunnelInfo? info = endpoint.tunnel;
  if (info == null) throw ArgumentError.value(endpoint, 'endpoint', 'not a tunnel endpoint');
  final Uri target = Uri(scheme: scheme, host: endpoint.host, port: endpoint.port == 443 && scheme == 'wss' ? null : endpoint.port, path: '/v1');
  final HttpClient client = HttpClient()..connectionTimeout = endpoint.timeout;
  try {
    final WebSocket socket = await WebSocket.connect(
      target.toString(),
      protocols: <String>[bridgeSubprotocol],
      headers: info.headersFor(target),
      customClient: client,
    ).timeout(endpoint.timeout + const Duration(seconds: 2));
    socket.pingInterval = const Duration(seconds: 20);
    return _IoWsConnection(socket, client);
  } on Object {
    client.close(force: true);
    rethrow;
  }
}

Future<WsConnection> connectPinned(Endpoint endpoint) async {
  bool mismatch = false;
  final HttpClient client = HttpClient(context: SecurityContext(withTrustedRoots: false))
    ..connectionTimeout = endpoint.timeout
    ..badCertificateCallback = (X509Certificate cert, String host, int port) {
      final bool ok = certificateMatchesFingerprint(cert.der, endpoint.fingerprint);
      if (!ok) mismatch = true;
      return ok;
    };
  final String host = endpoint.host.contains(':') ? '[${endpoint.host}]' : endpoint.host;
  try {
    final WebSocket socket = await WebSocket.connect(
      'wss://$host:${endpoint.port}/v1',
      protocols: <String>[bridgeSubprotocol],
      customClient: client,
    ).timeout(endpoint.timeout + const Duration(seconds: 2));
    socket.pingInterval = const Duration(seconds: 20);
    return _IoWsConnection(socket, client);
  } on Object {
    client.close(force: true);
    if (mismatch) throw const FingerprintMismatch();
    rethrow;
  }
}

class _IoWsConnection implements WsConnection {
  _IoWsConnection(this._socket, this._client);
  final WebSocket _socket;
  final HttpClient _client;

  @override
  Stream<Object> get messages => _socket.where((Object? m) => m != null).cast<Object>().map((Object m) {
        if (m is List<int> && m is! Uint8List) return Uint8List.fromList(m);
        return m;
      });

  @override
  void sendText(String text) => _socket.add(text);

  @override
  Future<void> close() async {
    await _socket.close();
    _client.close(force: true);
  }
}
