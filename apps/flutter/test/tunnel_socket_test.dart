import 'dart:async';
import 'dart:io';

import 'package:agent_console/core/net/tunnel.dart';
import 'package:agent_console/core/net/ws_transport.dart';
import 'package:flutter_test/flutter_test.dart';

// Kept apart from the widget tests: the Flutter test binding replaces dart:io sockets with stubs.
void main() {
  test('connectTunnel sends the Access headers on the upgrade request, with the bridge subprotocol', () async {
    final HttpServer server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    addTearDown(() => server.close(force: true));
    final Completer<HttpHeaders> seen = Completer<HttpHeaders>();
    unawaited(server.listen((HttpRequest request) async {
      seen.complete(request.headers);
      await WebSocketTransformer.upgrade(request, protocolSelector: (List<String> protocols) => protocols.first);
    }).asFuture<void>());

    // wss to a real host is not available in a unit test, so the same function runs over ws to loopback,
    // with the token's host set to that address.
    final TunnelInfo local = TunnelInfo(url: 'wss://127.0.0.1:${server.port}', accessClientId: 'cid', accessClientSecret: 'csecret');
    final WsConnection connection = await connectTunnel(Endpoint.viaTunnel(local), scheme: 'ws');
    final HttpHeaders headers = await seen.future;
    expect(headers.value('CF-Access-Client-Id'), 'cid');
    expect(headers.value('CF-Access-Client-Secret'), 'csecret');
    expect(headers.value('Sec-WebSocket-Protocol'), bridgeSubprotocol);
    await connection.close();
  });

  test('a LAN endpoint through the same server gets no Access headers', () async {
    // connectPinned has no header parameter at all; this checks the other half: the endpoints the profile
    // builds for LAN hosts have no tunnel, so connectEndpoint can only route them to connectPinned.
    const Endpoint lan = Endpoint(host: '10.0.0.1', port: 47821, fingerprint: 'aa');
    expect(lan.tunnel, isNull);
    expect(lan.kind, RouteKind.lan);
  });
}
