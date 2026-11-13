import 'dart:async';
import 'dart:typed_data';

import 'package:agent_console/core/net/bridge_client.dart';
import 'package:agent_console/core/net/ws_transport.dart';
import 'package:agent_console/core/pairing/mac_profile.dart';
import 'package:agent_console/core/protocol/models.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/fake_ws.dart';

const MacProfile _profile = MacProfile(
  id: 'm',
  name: 'Mac',
  hosts: <String>['10.0.0.1', '10.0.0.2'],
  port: 47821,
  fingerprint: '20d3c003d553a68bd969f5663f4b747be987a2874fe13bb025fe07710b829445',
  token: 'tok',
);

Future<void> _until(bool Function() condition, {Duration within = const Duration(seconds: 3)}) async {
  final DateTime deadline = DateTime.now().add(within);
  while (!condition()) {
    if (DateTime.now().isAfter(deadline)) fail('condition not met in $within');
    await Future<void>.delayed(const Duration(milliseconds: 5));
  }
}

BridgeClient _client(WsConnector connector, {void Function(Endpoint)? onGoodRoute, Duration requestTimeout = const Duration(seconds: 2)}) =>
    BridgeClient(
      connector: connector,
      profile: _profile,
      requestTimeout: requestTimeout,
      backoff: const <Duration>[Duration(milliseconds: 20), Duration(milliseconds: 40)],
      onGoodRoute: onGoodRoute,
    );

void main() {
  test('says hello with the token first, then answers requests by id', () async {
    final FakeWs socket = happyMac(auto: <String, Map<String, Object?>>{
      'sessions.list': <String, Object?>{'sessions': <Object?>[sessionJson('a'), sessionJson('b', state: 'needsInput')]},
    });
    final BridgeClient client = _client((Endpoint e) async => socket);
    client.start();
    await _until(() => client.isConnected);

    expect(socket.sent.first['op'], 'hello');
    expect(socket.sent.first['token'], 'tok');
    final List<Session> sessions = await client.listSessions();
    expect(sessions.map((Session s) => s.id), <String>['a', 'b']);
    expect(sessions.last.state, SessionState.needsInput);
    await client.dispose();
  });

  test('matches out-of-order responses to their requests', () async {
    final FakeWs socket = happyMac();
    final BridgeClient client = _client((Endpoint e) async => socket);
    client.start();
    await _until(() => client.isConnected);

    final Future<List<String>> projects = client.projectsRecent();
    final Future<List<Session>> sessions = client.listSessions();
    await _until(() => socket.sent.length >= 3);
    final Object projectsId = socket.sent[1]['id']!;
    final Object sessionsId = socket.sent[2]['id']!;
    expect(socket.sent[1]['op'], 'projects.recent');
    socket.reply(sessionsId, data: <String, Object?>{'sessions': <Object?>[sessionJson('z')]});
    socket.reply(projectsId, data: <String, Object?>{'projects': <Object?>['/a', '/b']});
    expect(await projects, <String>['/a', '/b']);
    expect((await sessions).single.id, 'z');
    await client.dispose();
  });

  test('error answers become BridgeException with the Mac code', () async {
    final FakeWs socket = happyMac();
    final BridgeClient client = _client((Endpoint e) async => socket);
    client.start();
    await _until(() => client.isConnected);
    final Future<void> send = client.sessionMessage('s1', 'hi');
    await _until(() => socket.sent.length >= 2);
    socket.fail(socket.sent[1]['id']!, 'view_only', 'Session mở ngoài Console — chỉ xem.');
    await expectLater(send, throwsA(isA<BridgeException>().having((BridgeException e) => e.isViewOnly, 'view_only', isTrue)));
    expect(socket.sent[1], containsPair('sessionId', 's1'));
    expect(socket.sent[1], containsPair('text', 'hi'));
    await client.dispose();
  });

  test('a request the Mac never answers times out', () async {
    final FakeWs socket = happyMac();
    final BridgeClient client = _client((Endpoint e) async => socket, requestTimeout: const Duration(milliseconds: 80));
    client.start();
    await _until(() => client.isConnected);
    await expectLater(client.ping(), throwsA(isA<BridgeException>().having((BridgeException e) => e.code, 'code', 'timeout')));
    await client.dispose();
  });

  test('requests fail fast while disconnected', () async {
    final BridgeClient client = _client((Endpoint e) async => throw Exception('unreachable'));
    await expectLater(client.ping(), throwsA(isA<BridgeException>().having((BridgeException e) => e.code, 'code', 'disconnected')));
    await client.dispose();
  });

  test('tries the hosts in order and reports the one that worked', () async {
    final List<String> tried = <String>[];
    String? good;
    final FakeWs socket = happyMac();
    final BridgeClient client = _client((Endpoint e) async {
      tried.add(e.host);
      if (e.host == '10.0.0.1') throw const FingerprintMismatch();
      return socket;
    }, onGoodRoute: (Endpoint route) => good = route.host);
    client.start();
    await _until(() => client.isConnected);
    expect(tried, <String>['10.0.0.1', '10.0.0.2']);
    expect(good, '10.0.0.2');
    await client.dispose();
  });

  test('reconnects after a drop, says hello again and re-subscribes', () async {
    final List<FakeWs> sockets = <FakeWs>[];
    final BridgeClient client = _client((Endpoint e) async {
      final FakeWs socket = happyMac(auto: <String, Map<String, Object?>>{'subscribe': <String, Object?>{'subscribed': true}});
      sockets.add(socket);
      return socket;
    });
    client.start();
    await _until(() => client.isConnected);
    await client.subscribe(const SubscriptionSpec(sessions: true, sessionId: 's1'));
    expect(sockets.first.sent.last, containsPair('sessions', true));

    final List<LinkPhase> phases = <LinkPhase>[];
    final StreamSubscription<ConnectionStatus> watching = client.status.listen((ConnectionStatus s) => phases.add(s.phase));
    await sockets.first.drop();
    await _until(() => sockets.length == 2 && client.isConnected);

    expect(phases, containsAllInOrder(<LinkPhase>[LinkPhase.disconnected, LinkPhase.connecting, LinkPhase.connected]));
    expect(sockets[1].sent.first['op'], 'hello');
    await _until(() => sockets[1].sent.length >= 2);
    expect(sockets[1].sent[1]['op'], 'subscribe');
    expect(sockets[1].sent[1], containsPair('sessionId', 's1'));
    await watching.cancel();
    await client.dispose();
  });

  test('pending requests fail when the link drops', () async {
    final FakeWs socket = happyMac();
    final BridgeClient client = _client((Endpoint e) async => socket);
    client.start();
    await _until(() => client.isConnected);
    final Future<void> pending = client.ping();
    await _until(() => socket.sent.length >= 2);
    await socket.drop();
    await expectLater(pending, throwsA(isA<BridgeException>().having((BridgeException e) => e.code, 'code', 'disconnected')));
    await client.dispose();
  });

  test('a revoked token stops the reconnecting', () async {
    int connects = 0;
    final BridgeClient client = _client((Endpoint e) async {
      connects++;
      return FakeWs(onSend: (FakeWs socket, Map<String, Object?> message) => socket.fail(message['id']!, 'unauthorized'));
    });
    client.start();
    await _until(() => client.currentStatus.revoked);
    await Future<void>.delayed(const Duration(milliseconds: 150));
    expect(connects, 1, reason: 'the first host already said unauthorized: no other host, no retry');
    await client.dispose();
  });

  test('parses pushed events', () async {
    final FakeWs socket = happyMac();
    final BridgeClient client = _client((Endpoint e) async => socket);
    final List<BridgeEvent> events = <BridgeEvent>[];
    client.events.listen(events.add);
    client.start();
    await _until(() => client.isConnected);

    socket.event('sessions', <String, Object?>{'data': <Object?>[sessionJson('a')]});
    socket.event('chat', <String, Object?>{
      'sessionId': 'a',
      'messages': <Object?>[<String, Object?>{'id': 'm1', 'role': 'assistant', 'text': 'xin chào', 'time': 1793541465000.0}],
    });
    socket.event('prompt', <String, Object?>{
      'sessionId': 'a',
      'prompt': <String, Object?>{'requestId': 'r1', 'sessionId': 'a', 'kind': 'permission', 'toolName': 'Bash', 'summary': 'ls', 'questions': <Object?>[]},
    });
    socket.event('prompt', <String, Object?>{'requestId': 'r1', 'resolved': true});
    socket.event('open', <String, Object?>{});
    await _until(() => events.length == 5);

    expect((events[0] as SessionsEvent).sessions.single.id, 'a');
    expect((events[1] as ChatEvent).messages.single.text, 'xin chào');
    expect((events[2] as PromptEvent).prompt!.toolName, 'Bash');
    expect((events[3] as PromptEvent).resolvedRequestId, 'r1');
    expect(events[4], isA<OpenEvent>());
    await client.dispose();
  });

  test('parses PHN1 phone frames and skips short or unknown binaries', () async {
    final FakeWs socket = happyMac();
    final BridgeClient client = _client((Endpoint e) async => socket);
    final List<PhoneFrame> frames = <PhoneFrame>[];
    client.phoneFrames.listen(frames.add);
    client.start();
    await _until(() => client.isConnected);

    socket.binary('PHN1', 0x01020304, <int>[0xFF, 0xD8, 0xFF, 0xD9]);
    socket.incoming.add(Uint8List.fromList(<int>[1, 2, 3]));
    socket.binary('XXXX', 1, <int>[9]);
    await _until(() => frames.isNotEmpty);
    await Future<void>.delayed(const Duration(milliseconds: 30));

    expect(frames, hasLength(1));
    expect(frames.single.sequence, 0x01020304);
    expect(frames.single.jpeg, <int>[0xFF, 0xD8, 0xFF, 0xD9]);
    await client.dispose();
  });

  test('web.snapshot pairs the JSON answer with the WEB1 frame of the same request', () async {
    final FakeWs socket = happyMac();
    final BridgeClient client = _client((Endpoint e) async => socket);
    client.start();
    await _until(() => client.isConnected);

    final Future<Uint8List> snapshot = client.webSnapshot('s1', url: 'http://localhost:3000', width: 300);
    await _until(() => socket.sent.length >= 2);
    final Map<String, Object?> request = socket.sent[1];
    expect(request['op'], 'web.snapshot');
    expect(request['url'], 'http://localhost:3000');
    socket.reply(request['id']!, data: <String, Object?>{'binary': true, 'bytes': 3});
    socket.binary('WEB1', 999, <int>[0]);
    socket.binary('WEB1', request['id']! as int, <int>[0xFF, 0xD8, 0xFF]);
    expect(await snapshot, <int>[0xFF, 0xD8, 0xFF]);
    await client.dispose();
  });

  test('typed ops send exactly the protocol parameters', () async {
    final FakeWs socket = happyMac(auto: <String, Map<String, Object?>>{
      'chat.get': <String, Object?>{'messages': <Object?>[], 'hasMore': true},
      'prompt.answer': <String, Object?>{'answered': true},
      'session.new': <String, Object?>{'started': true},
      'viz.get': <String, Object?>{'suggested': 'flow'},
    });
    final BridgeClient client = _client((Endpoint e) async => socket);
    client.start();
    await _until(() => client.isConnected);

    final ({List<ChatMessage> messages, bool hasMore, bool reset}) page = await client.chatGet('s1', limit: 20, before: 'm9');
    expect(page.hasMore, isTrue);
    await client.promptAllow('r1', allow: false);
    await client.promptChoose('r2', <Object>[2]);
    await client.promptChoose('r3', <Object>['a', 'b']);
    await client.sessionNew('/Users/me/app', model: 'opus');
    await client.vizGet('s1', entity: true);

    Map<String, Object?> op(String name) => socket.sent.firstWhere((Map<String, Object?> m) => m['op'] == name);
    expect(op('chat.get'), allOf(containsPair('sessionId', 's1'), containsPair('limit', 20), containsPair('before', 'm9')));
    expect(socket.sent.where((Map<String, Object?> m) => m['op'] == 'prompt.answer').map((Map<String, Object?> m) => m['allow'] ?? m['choice']).toList(),
        <Object?>[false, 2, <Object?>['a', 'b']]);
    expect(op('session.new'), allOf(containsPair('cwd', '/Users/me/app'), containsPair('model', 'opus')));
    expect(op('viz.get'), containsPair('entity', true));
    await client.dispose();
  });

  test('pairWithMac sends the code and returns the token, trying hosts in order', () async {
    final List<String> tried = <String>[];
    final String? hostThatWorked;
    final ({String token, String host, bool viaTunnel}) result = await pairWithMac(
      connector: (Endpoint e) async {
        tried.add(e.host);
        if (e.host == 'down.local') throw Exception('refused');
        return FakeWs(onSend: (FakeWs socket, Map<String, Object?> message) {
          expect(message['op'], 'pair');
          expect(message['code'], 'XRSCD75H');
          expect((message['device']! as Map<Object?, Object?>)['name'], 'p5-iphone-sim');
          socket.reply(message['id']!, data: <String, Object?>{'token': 'T' * 43, 'deviceId': 'd'});
        });
      },
      hosts: <String>['down.local', '127.0.0.1'],
      port: 47821,
      fingerprint: _profile.fingerprint,
      code: 'XRSCD75H',
      deviceName: 'p5-iphone-sim',
      platform: 'ios',
    );
    hostThatWorked = result.host;
    expect(tried, <String>['down.local', '127.0.0.1']);
    expect(hostThatWorked, '127.0.0.1');
    expect(result.token, hasLength(43));
  });

  test('pairWithMac surfaces a denial and never tries another host after a Mac answer', () async {
    int connects = 0;
    await expectLater(
      pairWithMac(
        connector: (Endpoint e) async {
          connects++;
          return FakeWs(onSend: (FakeWs socket, Map<String, Object?> message) => socket.fail(message['id']!, 'unauthorized', 'Mac đã từ chối thiết bị này.'));
        },
        hosts: <String>['a', 'b'],
        port: 1,
        fingerprint: _profile.fingerprint,
        code: 'XRSCD75H',
        deviceName: 'x',
        platform: 'ios',
      ),
      throwsA(isA<BridgeException>().having((BridgeException e) => e.isUnauthorized, 'unauthorized', isTrue)),
    );
    expect(connects, 1);
  });

  test('pairWithMac rethrows a fingerprint mismatch', () async {
    await expectLater(
      pairWithMac(
        connector: (Endpoint e) async => throw const FingerprintMismatch(),
        hosts: <String>['a'],
        port: 1,
        fingerprint: _profile.fingerprint,
        code: 'XRSCD75H',
        deviceName: 'x',
        platform: 'ios',
      ),
      throwsA(isA<FingerprintMismatch>()),
    );
  });
}
