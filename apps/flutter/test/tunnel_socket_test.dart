import 'dart:async';
import 'dart:io';

import 'package:agent_console/core/net/tunnel.dart';
import 'package:agent_console/core/net/ws_transport.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('connectTunnel sends the Access headers on the upgrade request, with the bridge subprotocol', () async {
    final HttpServer server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    addTearDown(() => server.close(force: true));
    final Completer<HttpHeaders> seen = Completer<HttpHeaders>();
    unawaited(server.listen((HttpRequest request) async {
      seen.complete(request.headers);
      await WebSocketTransformer.upgrade(request, protocolSelector: (List<String> protocols) => protocols.first);
    }).asFuture<void>());

    final TunnelInfo local = TunnelInfo(url: 'wss://127.0.0.1:${server.port}', accessClientId: 'cid', accessClientSecret: 'csecret');
    final WsConnection connection = await connectTunnel(Endpoint.viaTunnel(local), scheme: 'ws');
    final HttpHeaders headers = await seen.future;
    expect(headers.value('CF-Access-Client-Id'), 'cid');
    expect(headers.value('CF-Access-Client-Secret'), 'csecret');
    expect(headers.value('Sec-WebSocket-Protocol'), bridgeSubprotocol);
    await connection.close();
  });

  test('a LAN endpoint through the same server gets no Access headers', () async {
    const Endpoint lan = Endpoint(host: '10.0.0.1', port: 47821, fingerprint: 'aa');
    expect(lan.tunnel, isNull);
    expect(lan.kind, RouteKind.lan);
  });
}
