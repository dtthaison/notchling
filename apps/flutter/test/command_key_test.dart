import 'package:agent_console/core/net/bridge_client.dart';
import 'package:agent_console/core/net/command_key.dart';
import 'package:agent_console/core/net/ws_transport.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/fake_ws.dart';
import 'support/harness.dart';

void main() {
  test('a command key is a version 4 UUID and differs each time', () {
    final RegExp uuid = RegExp(r'^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$');
    final Set<String> keys = <String>{for (int i = 0; i < 50; i++) newCommandKey()};
    expect(keys.length, 50);
    expect(keys.every(uuid.hasMatch), isTrue);
  });

  test('the same key goes with every try of one command', () async {
    final List<FakeWs> sockets = <FakeWs>[];
    final BridgeClient client = BridgeClient(
      connector: (Endpoint e) async {
        final FakeWs socket = happyMac(auto: <String, Map<String, Object?>>{'session.message': <String, Object?>{'sent': true}});
        sockets.add(socket);
        return socket;
      },
      profile: testMac,
      backoff: const <Duration>[Duration(milliseconds: 20)],
    );
    client.start();
    await until(() => client.isConnected);
    await client.sessionMessage('s1', 'hi', key: 'k-1');
    await sockets.first.drop();
    await until(() => sockets.length == 2 && client.isConnected);
    await client.sessionMessage('s1', 'hi', key: 'k-1');
    await client.sessionMessage('s1', 'other');
    final List<Object?> keys = <Object?>[
      for (final FakeWs socket in sockets)
        for (final Map<String, Object?> m in socket.sent)
          if (m['op'] == 'session.message') m['key'],
    ];
    expect(keys.take(2), <Object?>['k-1', 'k-1']);
    expect(keys.last, isA<String>().having((String k) => k == 'k-1', 'is another command', isFalse));
    await client.dispose();
  });

  test('an address with no port is skipped, never given one', () {
    expect(splitAddress('192.168.1.6:47821'), (host: '192.168.1.6', port: 47821));
    expect(splitAddress('m1.local:5'), (host: 'm1.local', port: 5));
    expect(splitAddress('[fe80::1]:47821'), (host: 'fe80::1', port: 47821));
    expect(splitAddress('192.168.1.6'), isNull);
    expect(splitAddress('fe80::1'), isNull);
    expect(splitAddress('host:'), isNull);
    expect(splitAddress('host:0'), isNull);
    expect(splitAddress(':47821'), isNull);
  });
}
