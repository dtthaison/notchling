import 'dart:async';
import 'dart:io';

import 'package:agent_console/app/shell.dart';
import 'package:agent_console/core/net/bridge_client.dart';
import 'package:agent_console/core/net/tunnel.dart';
import 'package:agent_console/core/net/ws_transport.dart';
import 'package:agent_console/core/pairing/mac_profile.dart';
import 'package:agent_console/core/pairing/pairing_payload.dart';
import 'package:agent_console/features/connection/connection.dart';
import 'package:agent_console/features/macs/macs_notifier.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:flutter_test/flutter_test.dart';

import 'support/fake_ws.dart';
import 'support/harness.dart';

const String _fp = '20d3c003d553a68bd969f5663f4b747be987a2874fe13bb025fe07710b829445';
const TunnelInfo _tunnel = TunnelInfo(url: 'wss://console.trietkun.shop', accessClientId: 'id.access', accessClientSecret: 'shh');

const MacProfile _lanOnly =
    MacProfile(id: 'm', name: 'Mac', hosts: <String>['10.0.0.1', 'mac.local'], port: 47821, fingerprint: _fp, token: 'tok');
const MacProfile _withTunnel = MacProfile(
    id: 'm', name: 'Mac', hosts: <String>['10.0.0.1', 'mac.local'], port: 47821, fingerprint: _fp, token: 'tok', tunnel: _tunnel);

String _qr({Object? tunnel, int version = 1}) => '{"v":$version,"name":"Mac","port":47821,"fp":"$_fp","hosts":["10.0.0.1"],"code":"ABCD2345"'
    '${tunnel == null ? '' : ',"tunnel":$tunnel'}}';

const String _tunnelJson = '{"url":"wss://console.trietkun.shop","accessClientId":"id.access","accessClientSecret":"shh"}';

List<String> _order(List<Endpoint> routes) => routes.map((Endpoint e) => e.kind == RouteKind.tunnel ? 'tunnel' : e.host).toList();

void main() {
  group('QR with a tunnel block', () {
    test('a v1 QR without a tunnel still parses and has none', () {
      expect(PairingPayload.parse(_qr()).tunnel, isNull);
    });

    test('the tunnel block is read, whether the QR says v1 or v2', () {
      for (final int version in <int>[1, 2]) {
        final TunnelInfo? tunnel = PairingPayload.parse(_qr(tunnel: _tunnelJson, version: version)).tunnel;
        expect(tunnel?.url, 'wss://console.trietkun.shop');
        expect(tunnel?.accessClientId, 'id.access');
        expect(tunnel?.accessClientSecret, 'shh');
      }
    });

    test('a tunnel that is not wss, or lacks a token part, is refused', () {
      expect(() => PairingPayload.parse(_qr(tunnel: '{"url":"ws://x.shop","accessClientId":"a","accessClientSecret":"b"}')),
          throwsA(isA<PairingFormatException>()));
      expect(() => PairingPayload.parse(_qr(tunnel: '{"url":"wss://x.shop","accessClientId":"","accessClientSecret":"b"}')),
          throwsA(isA<PairingFormatException>()));
      expect(() => PairingPayload.parse(_qr(tunnel: '{"url":"wss://x.shop"}')), throwsA(isA<PairingFormatException>()));
    });

    test('a Mac with a tunnel survives the secure-storage round trip; an old stored Mac loads without one', () {
      const MacProfile saved = MacProfile(
          id: 'm', name: 'Mac', hosts: <String>['h'], port: 1, fingerprint: _fp, token: 't', tunnel: _tunnel, lastGoodWasTunnel: true);
      final MacProfile loaded = decodeProfiles(encodeProfiles(<MacProfile>[saved])).single;
      expect(loaded.tunnel?.accessClientSecret, 'shh');
      expect(loaded.lastGoodWasTunnel, isTrue);

      final MacProfile old = decodeProfiles('[{"id":"a","name":"n","hosts":["h"],"port":1,"fp":"$_fp","token":"t"}]').single;
      expect(old.tunnel, isNull);
      expect(old.lastGoodWasTunnel, isFalse);
    });
  });

  group('route order', () {
    test('without a tunnel it is the LAN hosts, last good first', () {
      expect(_order(_lanOnly.endpoints()), <String>['10.0.0.1', 'mac.local']);
      expect(_order(_lanOnly.withLastGoodHost('mac.local').endpoints()), <String>['mac.local', '10.0.0.1']);
    });

    test('the tunnel comes after every LAN host', () {
      expect(_order(_withTunnel.endpoints()), <String>['10.0.0.1', 'mac.local', 'tunnel']);
    });

    test('the route that worked last goes first, LAN or tunnel', () {
      expect(_order(_withTunnel.withLastGoodHost('mac.local').endpoints()), <String>['mac.local', '10.0.0.1', 'tunnel']);
      expect(_order(_withTunnel.withLastGoodTunnel().endpoints()), <String>['tunnel', '10.0.0.1', 'mac.local']);
    });

    test('after a network change the LAN is tried first even if the tunnel worked last', () {
      expect(_order(_withTunnel.withLastGoodTunnel().endpoints(preferLan: true)), <String>['10.0.0.1', 'mac.local', 'tunnel']);
    });

    test('LAN routes use pinned TLS and carry no token; only the tunnel route has one', () {
      for (final Endpoint route in _withTunnel.endpoints()) {
        if (route.kind == RouteKind.lan) {
          expect(route.tunnel, isNull);
          expect(route.fingerprint, _fp);
        } else {
          expect(route.tunnel?.accessClientId, 'id.access');
          expect(route.host, 'console.trietkun.shop');
          expect(route.port, 443);
        }
      }
    });
  });

  group('Access headers', () {
    test('are produced only for the tunnel host', () {
      expect(_tunnel.headersFor(Uri.parse('wss://console.trietkun.shop/v1')),
          <String, String>{'CF-Access-Client-Id': 'id.access', 'CF-Access-Client-Secret': 'shh'});
      expect(_tunnel.headersFor(Uri.parse('wss://CONSOLE.trietkun.shop/v1')), isNotEmpty, reason: 'host names are case-insensitive');
      expect(_tunnel.headersFor(Uri.parse('wss://10.0.0.1:47821/v1')), isEmpty);
      expect(_tunnel.headersFor(Uri.parse('wss://mac.local:47821/v1')), isEmpty);
      expect(_tunnel.headersFor(Uri.parse('wss://evil.example/v1')), isEmpty);
      expect(_tunnel.headersFor(Uri.parse('wss://console.trietkun.shop.evil.example/v1')), isEmpty);
    });

    test('connectTunnel refuses an endpoint that is not a tunnel', () {
      expect(connectTunnel(const Endpoint(host: '10.0.0.1', port: 1, fingerprint: _fp)), throwsA(isA<ArgumentError>()));
    });
  });

  group('fallback over the client', () {
    BridgeClient clientFor(MacProfile profile, WsConnector connector, {void Function(Endpoint)? onGoodRoute}) => BridgeClient(
          connector: connector,
          profile: profile,
          backoff: const <Duration>[Duration(milliseconds: 20)],
          onGoodRoute: onGoodRoute,
        );

    test('LAN down, so it moves on to the tunnel, reports it, and says it is on the Internet', () async {
      final List<String> tried = <String>[];
      Endpoint? good;
      final BridgeClient client = clientFor(_withTunnel, (Endpoint e) async {
        tried.add(e.kind == RouteKind.tunnel ? 'tunnel' : e.host);
        if (e.kind == RouteKind.lan) throw const SocketException('no route to host');
        return happyMac();
      }, onGoodRoute: (Endpoint route) => good = route);
      client.start();
      await until(() => client.isConnected);
      expect(tried, <String>['10.0.0.1', 'mac.local', 'tunnel']);
      expect(good?.kind, RouteKind.tunnel);
      expect(client.currentStatus.route, RouteKind.tunnel);
      await client.dispose();
    });

    test('with the LAN reachable the tunnel is never contacted', () async {
      final List<RouteKind> kinds = <RouteKind>[];
      final BridgeClient client = clientFor(_withTunnel, (Endpoint e) async {
        kinds.add(e.kind);
        return happyMac();
      });
      client.start();
      await until(() => client.isConnected);
      expect(kinds, <RouteKind>[RouteKind.lan]);
      expect(client.currentStatus.route, RouteKind.lan);
      await client.dispose();
    });

    test('every route failing leaves the link down with the last error, and retries on its own', () async {
      int attempts = 0;
      final BridgeClient client = clientFor(_withTunnel, (Endpoint e) async {
        attempts++;
        throw const SocketException('down');
      });
      client.start();
      await until(() => attempts >= 6);
      expect(client.isConnected, isFalse);
      expect(client.currentStatus.phase, isNot(LinkPhase.connected));
      await client.dispose();
    });

    test('a network change moves a tunnel link back to the home network', () async {
      bool atHome = false;
      final List<String> tried = <String>[];
      final BridgeClient client = clientFor(_withTunnel.withLastGoodTunnel(), (Endpoint e) async {
        tried.add(e.kind == RouteKind.tunnel ? 'tunnel' : e.host);
        if (e.kind == RouteKind.lan && !atHome) throw const SocketException('not home');
        return happyMac();
      });
      client.start();
      await until(() => client.isConnected);
      expect(client.currentStatus.route, RouteKind.tunnel);
      expect(tried, <String>['tunnel'], reason: 'the route that worked last goes first');

      atHome = true;
      tried.clear();
      client.networkChanged();
      await until(() => client.isConnected && client.currentStatus.route == RouteKind.lan);
      expect(tried.first, '10.0.0.1', reason: 'LAN first after a network change');
      await client.dispose();
    });

    test('a network change away from home falls through to the tunnel', () async {
      bool atHome = true;
      final BridgeClient client = clientFor(_withTunnel, (Endpoint e) async {
        if (e.kind == RouteKind.lan && !atHome) throw const SocketException('not home');
        return happyMac();
      });
      client.start();
      await until(() => client.isConnected && client.currentStatus.route == RouteKind.lan);
      atHome = false;
      client.networkChanged();
      await until(() => client.isConnected && client.currentStatus.route == RouteKind.tunnel);
      await client.dispose();
    });

    test('the provider listens to network changes and remembers the route that worked', () async {
      final StreamController<void> changes = StreamController<void>.broadcast();
      addTearDown(changes.close);
      bool atHome = false;
      final ProviderContainer container = containerWith(
        happyMac(),
        mac: _withTunnel,
        networkChanges: changes.stream,
        connector: (Endpoint e) async {
          if (e.kind == RouteKind.lan && !atHome) throw const SocketException('not home');
          return happyMac();
        },
      );
      await connect(container);
      expect(container.read(macsProvider).active?.lastGoodWasTunnel, isTrue);

      atHome = true;
      changes.add(null);
      await until(() => container.read(connectionStatusProvider).route == RouteKind.lan);
      await until(() => container.read(macsProvider).active?.lastGoodWasTunnel == false);
      expect(container.read(macsProvider).active?.lastGoodHost, '10.0.0.1');
    });
  });

  group('pairing over the tunnel', () {
    test('falls back to the tunnel when no LAN host answers, and says so', () async {
      final List<RouteKind> tried = <RouteKind>[];
      final ({String token, String host, bool viaTunnel}) result = await pairWithMac(
        connector: (Endpoint e) async {
          tried.add(e.kind);
          if (e.kind == RouteKind.lan) throw const SocketException('down');
          return FakeWs(onSend: (FakeWs socket, Map<String, Object?> message) {
            socket.reply(message['id']!, data: <String, Object?>{'token': 'T' * 43, 'deviceId': 'd'});
          });
        },
        hosts: <String>['10.0.0.1'],
        port: 47821,
        fingerprint: _fp,
        code: 'ABCD2345',
        deviceName: 'phone',
        platform: 'android',
        tunnel: _tunnel,
      );
      expect(tried, <RouteKind>[RouteKind.lan, RouteKind.tunnel]);
      expect(result.viaTunnel, isTrue);
      expect(result.host, 'console.trietkun.shop');
    });
  });

  group('banner', () {
    Future<void> show(WidgetTester tester, ConnectionStatus status) async {
      await tester.pumpWidget(ProviderScope(
        overrides: <Override>[connectionStatusProvider.overrideWith(() => _FixedStatus(status))],
        child: host(const ConnectionBanner()),
      ));
      await tester.pump();
    }

    testWidgets('says Internet while the link runs through the tunnel, and keeps saying it', (WidgetTester tester) async {
      await show(tester, const ConnectionStatus(LinkPhase.connected, route: RouteKind.tunnel));
      await tester.pump(const Duration(seconds: 3));
      expect(find.text('Đã kết nối · Internet'), findsOneWidget);
    });

    testWidgets('on the home network it shows Wi-Fi nhà briefly, then goes quiet', (WidgetTester tester) async {
      await show(tester, const ConnectionStatus(LinkPhase.connecting));
      await tester.pumpWidget(ProviderScope(
        overrides: <Override>[connectionStatusProvider.overrideWith(() => _FixedStatus(const ConnectionStatus(LinkPhase.connected, route: RouteKind.lan)))],
        child: host(const ConnectionBanner()),
      ));
      await tester.pump();
      await tester.pump(const Duration(seconds: 3));
      expect(find.textContaining('Internet'), findsNothing);
      expect(find.text('Đã kết nối · Wi-Fi nhà'), findsNothing, reason: 'hidden after two seconds');
    });

    testWidgets('when no route works it explains what to do', (WidgetTester tester) async {
      await show(tester, ConnectionStatus(LinkPhase.disconnected, error: const SocketException('down'), retryAt: DateTime.now().add(const Duration(seconds: 5))));
      expect(find.text('Không tới được Mac. Bật Truy cập qua internet trên Mac hoặc vào cùng Wi-Fi.'), findsOneWidget);
      expect(find.text('Thử lại'), findsOneWidget);
    });

    testWidgets('a plain drop keeps the retry countdown message', (WidgetTester tester) async {
      await show(tester, ConnectionStatus(LinkPhase.disconnected, retryAt: DateTime.now().add(const Duration(seconds: 5))));
      expect(find.textContaining('Mất kết nối'), findsOneWidget);
    });
  });
}

class _FixedStatus extends ConnectionStatusNotifier {
  _FixedStatus(this._status);
  final ConnectionStatus _status;

  @override
  ConnectionStatus build() => _status;
}
