import 'dart:async';

import 'package:agent_console/core/net/bridge_client.dart';
import 'package:agent_console/core/net/ws_transport.dart';
import 'package:agent_console/core/pairing/mac_profile.dart';
import 'package:agent_console/features/macs/macs_notifier.dart';
import 'package:agent_console/app/widgets/common.dart';
import 'package:agent_console/core/protocol/generated.dart' as wire;
import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/core/net/tunnel.dart';
import 'package:agent_console/features/chat/chat_notifier.dart';
import 'package:agent_console/features/chat/chat_parts.dart';
import 'package:agent_console/features/connection/connection.dart';
import 'package:agent_console/features/sessions/held_notifier.dart';
import 'package:agent_console/features/sessions/held_section.dart';
import 'package:agent_console/features/sessions/new_session_sheet.dart';
import 'package:agent_console/features/sessions/sessions_notifier.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/cluster_world.dart';
import 'support/fake_ws.dart';
import 'support/harness.dart';

class _BrokenWs extends FakeWs {
  _BrokenWs({super.onSend});
  bool broken = false;

  @override
  void sendText(String text) {
    if (broken) throw StateError('socket closed');
    super.sendText(text);
  }
}

Iterable<Map<String, Object?>> _ops(Iterable<FakeWs> sockets, String op) =>
    <Map<String, Object?>>[for (final FakeWs s in sockets) for (final Map<String, Object?> m in s.sent) if (m['op'] == op) m];

void main() {
  group('a dead link is noticed', () {
    test('a Standby that dies while the primary is being probed is noticed at once', () async {
      final ClusterWorld world = ClusterWorld();
      final BridgeClient client = clusterClient(world, primaryRetry: const Duration(milliseconds: 30));
      client.start();
      await until(() => client.isConnected);
      await world.primaryGoesDown();
      await viaM1(client);

      world.hangPrimary = true;
      world.m5Up = true;
      world.dialed.clear();
      await until(() => world.dialed.contains('10.0.0.5:47821'));
      await world.liveM1!.drop();
      await until(() => !client.isConnected, within: const Duration(milliseconds: 500));
      await client.dispose();
    });

    test('a send on a socket that throws is a disconnected error, not a crash', () async {
      final _BrokenWs socket = _BrokenWs(onSend: (FakeWs s, Map<String, Object?> m) {
        if (m['op'] == 'hello') s.reply(m['id']!, data: <String, Object?>{'version': 1});
      });
      final BridgeClient client = BridgeClient(connector: (Endpoint e) async => socket, profile: testMac, backoff: const <Duration>[Duration(seconds: 5)]);
      client.start();
      await until(() => client.isConnected);
      socket.broken = true;
      await expectLater(client.sessionMessage('s1', 'hi'), throwsA(isA<BridgeException>().having((BridgeException e) => e.code, 'code', 'disconnected')));
      await client.dispose();
    });
  });

  group('stay with the machine that holds my commands', () {
    test('a reconnect tries the holder first, and the paired Mac once nothing is held', () async {
      final ClusterWorld world = ClusterWorld();
      final BridgeClient client = clusterClient(world);
      client.start();
      await until(() => client.isConnected);
      await world.primaryGoesDown();
      await viaM1(client);
      client.markHeld(true);
      expect(client.holding, isTrue);

      world.m5Up = true;
      world.dialed.clear();
      client.networkChanged();
      await until(() => world.dialed.isNotEmpty && client.isConnected);
      expect(world.dialed.first, '10.0.0.6:47821');
      expect(client.currentStatus.via, 'm1');

      client.markHeld(false);
      world.dialed.clear();
      client.networkChanged();
      await until(() => world.dialed.isNotEmpty && client.isConnected && client.currentStatus.via == null);
      expect(world.dialed.first, '10.0.0.5:47821');
      await client.dispose();
    });

    test('held state is kept per computer, and a failed load clears it', () async {
      final ClusterWorld world = ClusterWorld()..held.add(heldEntry('h-1', 'one'));
      final ClusterWorld other = ClusterWorld()..held.add(heldEntry('h-o', 'two'));
      final MacProfile second = m5Profile.withCluster('m5', pinnedM5().nodes, term: 3);
      final MacProfile otherMac = MacProfile(
        id: 'o9',
        name: 'o9',
        hosts: const <String>['10.0.0.9'],
        port: 47821,
        fingerprint: fpM5,
        token: 'tok2',
        nodes: second.nodes,
        clusterPrimary: 'm5',
        clusterTerm: 3,
      );
      final ProviderContainer container = clusterContainerWith(
        world,
        pinnedM5(),
        others: <MacProfile>[otherMac],
        connector: (Endpoint e) => e.host == '10.0.0.9' ? other.connect(Endpoint(host: '10.0.0.5', port: 47821, fingerprint: fpM5)) : world.connect(e),
      );
      container.read(macsProvider);
      await until(() => container.read(macsProvider).loaded);
      container.listen<HeldState>(computerHeldProvider('o9'), (HeldState? a, HeldState b) {});
      container.listen<HeldState>(computerHeldProvider('m5'), (HeldState? a, HeldState b) {});
      await until(() => container.read(computerHeldProvider('o9')).entries.isNotEmpty);
      await until(() => container.read(computerHeldProvider('m5')).entries.isNotEmpty);
      expect(container.read(computerClientProvider('o9'))!.holding, isTrue);
      expect(container.read(computerClientProvider('m5'))!.holding, isTrue);

      world.heldListError = 'invalid';
      await container.read(computerHeldProvider('m5').notifier).reload();
      expect(container.read(computerClientProvider('m5'))!.holding, isFalse);
      expect(container.read(computerHeldProvider('m5')).entries, isEmpty);
      expect(container.read(computerClientProvider('o9'))!.holding, isTrue, reason: 'another computer is not touched');
    });
  });

  group('retry keys', () {
    test('answering the same prompt again keeps its key', () async {
      final FakeWs socket = FakeWs(onSend: (FakeWs s, Map<String, Object?> m) {
        switch (m['op']) {
          case 'hello':
            s.reply(m['id']!, data: <String, Object?>{'version': 1});
          case 'prompt.answer':
            final bool first = s.sent.where((Map<String, Object?> x) => x['op'] == 'prompt.answer').length == 1;
            if (first) {
              s.fail(m['id']!, 'internal');
            } else {
              s.reply(m['id']!, data: <String, Object?>{'answered': true});
            }
        }
      });
      final ProviderContainer container = containerWith(socket);
      await connect(container);
      container.listen<ChatState>(chatProvider('s1'), (ChatState? a, ChatState b) {});
      const PendingPrompt prompt = PendingPrompt(requestId: 'r1', sessionId: 's1', isPermission: true, toolName: 'Bash', summary: 'ls', questions: <PromptQuestion>[]);
      await container.read(chatProvider('s1').notifier).allow(prompt, allow: true);
      await container.read(chatProvider('s1').notifier).allow(prompt, allow: true);
      final List<Map<String, Object?>> answers = _ops(<FakeWs>[socket], 'prompt.answer').toList();
      expect(answers, hasLength(2));
      expect(answers[1]['key'], answers[0]['key']);
      const PendingPrompt other = PendingPrompt(requestId: 'r2', sessionId: 's1', isPermission: true, toolName: 'Bash', summary: 'pwd', questions: <PromptQuestion>[]);
      await container.read(chatProvider('s1').notifier).allow(other, allow: true);
      expect(_ops(<FakeWs>[socket], 'prompt.answer').last['key'], isNot(answers[0]['key']));
    });

    testWidgets('send now again after a failure keeps its key', (WidgetTester tester) async {
      final FakeWs socket = FakeWs(onSend: (FakeWs s, Map<String, Object?> m) {
        switch (m['op']) {
          case 'hello':
            s.reply(m['id']!);
          case 'sessions.list':
            s.reply(m['id']!, data: <String, Object?>{'sessions': <Object?>[sessionJson('s')]});
          case 'subscribe':
            s.reply(m['id']!, data: <String, Object?>{'subscribed': true});
          case 'session.sendNow':
            final bool first = s.sent.where((Map<String, Object?> x) => x['op'] == 'session.sendNow').length == 1;
            if (first) {
              s.fail(m['id']!, 'internal');
            } else {
              s.reply(m['id']!, data: <String, Object?>{'sent': true});
            }
        }
      });
      final ProviderContainer container = containerWith(socket);
      await tester.runAsync(() async {
        await connect(container);
        container.listen<SessionsState>(sessionsProvider, (SessionsState? a, SessionsState b) {});
        await until(() => container.read(sessionsProvider).sessions.isNotEmpty);
      });
      final ChatMessage queued = ChatMessage(id: 'q', role: ChatRole.user, text: 'next', time: DateTime(2026), queued: true);
      await tester.pumpWidget(UncontrolledProviderScope(container: container, child: host(UserBubble(message: queued, sessionId: 's'))));
      for (int i = 0; i < 2; i++) {
        await tester.tap(find.text('Send now'));
        await tester.pumpAndSettle();
        await tester.tap(find.descendant(of: find.byType(AlertDialog), matching: find.text('Send now')));
        await tester.runAsync(() => until(() => _ops(<FakeWs>[socket], 'session.sendNow').length == i + 1));
        await tester.pumpAndSettle();
      }
      final List<Map<String, Object?>> sent = _ops(<FakeWs>[socket], 'session.sendNow').toList();
      expect(sent[1]['key'], sent[0]['key']);
      await tester.pumpWidget(const SizedBox());
      await tester.pump(const Duration(seconds: 9));
    });

    testWidgets('starting a session again after a failure keeps its key, and another folder or model is a new command', (WidgetTester tester) async {
      final FakeWs socket = FakeWs(onSend: (FakeWs s, Map<String, Object?> m) {
        switch (m['op']) {
          case 'hello':
            s.reply(m['id']!);
          case 'projects.recent':
            s.reply(m['id']!, data: <String, Object?>{'projects': <Object?>['/work/app']});
          case 'session.new':
            s.fail(m['id']!, 'internal');
        }
      });
      final ProviderContainer container = containerWith(socket);
      await tester.runAsync(() => connect(container));
      await tester.pumpWidget(UncontrolledProviderScope(
        container: container,
        child: host(Scaffold(body: Builder(builder: (BuildContext context) => TextButton(onPressed: () => unawaited(showNewSessionSheet(context)), child: const Text('open')))), screen: true),
      ));
      await tester.tap(find.text('open'));
      await tester.pump(const Duration(milliseconds: 400));
      await tester.runAsync(() => until(() => _ops(<FakeWs>[socket], 'projects.recent').isNotEmpty));
      await tester.pumpAndSettle();
      await tester.tap(find.text('/work/app'));
      await tester.pump();
      Future<void> start(int count) async {
        await tester.tap(find.widgetWithText(PrimaryButton, 'Start'));
        await tester.pumpAndSettle();
        await tester.tap(find.descendant(of: find.byType(AlertDialog), matching: find.text('Start')));
        await tester.runAsync(() => until(() => _ops(<FakeWs>[socket], 'session.new').length == count));
        await tester.pumpAndSettle();
      }

      await start(1);
      await start(2);
      await tester.tap(find.text('opus'));
      await tester.pump();
      await start(3);
      final List<Map<String, Object?>> sent = _ops(<FakeWs>[socket], 'session.new').toList();
      expect(sent[1]['key'], sent[0]['key']);
      expect(sent[2]['key'], isNot(sent[0]['key']));
      await tester.pumpWidget(const SizedBox());
    });
  });

  group('held actions', () {
    test('a cancel that did nothing is a failure, and one for a command already sent is not', () async {
      final ClusterWorld world = ClusterWorld()
        ..m5Up = false
        ..held.add(heldEntry('held-1', 'Run it'));
      final ProviderContainer container = clusterContainerWith(world, pinnedM5());
      await connectContainer(container);
      container.listen<HeldState>(computerHeldProvider('m5'), (HeldState? a, HeldState b) {});
      await until(() => container.read(computerHeldProvider('m5')).entries.isNotEmpty);
      expect(container.read(computerHeldProvider('m5')).holder, 'm1');

      world.cancelOverride = <String, Object?>{'cancelled': false};
      await container.read(computerHeldProvider('m5').notifier).cancel('held-1');
      expect(container.read(computerHeldProvider('m5')).failed, isTrue);

      world.cancelOverride = <String, Object?>{'cancelled': false, 'delivered': true};
      await container.read(computerHeldProvider('m5').notifier).cancel('held-1');
      expect(container.read(computerHeldProvider('m5')).failed, isFalse);
    });

    testWidgets('while the link is down the buttons are off, and each entry says who holds it', (WidgetTester tester) async {
      final List<String> pressed = <String>[];
      await tester.pumpWidget(host(HeldList(
        entries: <wire.HeldEntry>[
          wire.HeldEntry(id: 'a', op: 'session.message', summary: 'Run it', at: 1, waiting: false),
        ],
        notices: const <wire.HeldNotice>[],
        nodeName: 'm1',
        primaryAnswers: false,
        enabled: false,
        failed: true,
        onCancel: pressed.add,
        onConfirm: pressed.add,
        onDismiss: pressed.add,
      )));
      expect(find.text('Held on m1'), findsOneWidget);
      await tester.tap(find.text('Cancel'), warnIfMissed: false);
      expect(pressed, isEmpty);
      expect(find.text("Couldn't update that command. Try again."), findsOneWidget);
    });
  });

  group('pins are learned only over the paired Mac pinned route', () {
    test('a hello over a fallback machine adds no machine and changes no pin', () async {
      final ClusterWorld world = ClusterWorld();
      world.m1ClusterOverride = <String, Object?>{
        'primary': 'm5',
        'term': 3,
        'nodes': <Object?>[
          clusterNode('m5', 'm5', '10.0.0.5:47821', 'e' * 64),
          clusterNode('m1', 'm1', '10.0.0.6:47821', 'f' * 64),
          clusterNode('m9', 'm9', '10.0.0.9:47821', 'a' * 64),
        ],
      };
      final List<List<PinnedNode>> learned = <List<PinnedNode>>[];
      final BridgeClient client = clusterClient(world, profile: pinnedM5(), onCluster: (String? p, int t, List<PinnedNode> n) => learned.add(n));
      world.m5Up = false;
      client.start();
      await viaM1(client);
      await Future<void>.delayed(const Duration(milliseconds: 50));
      expect(learned, isEmpty, reason: 'nothing learned or changed from m1');
      await client.dispose();
    });

    test('a fallback machine moves the primary only with a higher term', () async {
      final ClusterWorld world = ClusterWorld();
      Map<String, Object?> cluster(String primary, int term) => <String, Object?>{
            'primary': primary,
            'term': term,
            'nodes': <Object?>[clusterNode('m5', 'm5', '10.0.0.5:47821', fpM5), clusterNode('m1', 'm1', '10.0.0.6:47821', fpM1)],
          };
      final List<(String?, int)> seen = <(String?, int)>[];
      world.m1ClusterOverride = cluster('m1', 2);
      world.m5Up = false;
      final BridgeClient client = clusterClient(world, profile: pinnedM5(), onCluster: (String? p, int t, List<PinnedNode> n) => seen.add((p, t)));
      client.start();
      await viaM1(client);
      expect(seen, isEmpty, reason: 'term 2 is older than the stored 3');
      await client.dispose();

      world.m1ClusterOverride = cluster('m1', 4);
      final BridgeClient newer = clusterClient(world, profile: pinnedM5(), onCluster: (String? p, int t, List<PinnedNode> n) => seen.add((p, t)));
      newer.start();
      await viaM1(newer);
      expect(seen, <(String?, int)>[('m1', 4)]);
      expect(newer.currentStatus.primaryName, isNull, reason: 'm1 is the primary now, so nothing is unreachable behind it');
      await newer.dispose();
    });

    test('a hello over the paired Mac tunnel replaces no pin', () async {
      final ClusterWorld world = ClusterWorld();
      final MacProfile viaTunnel = MacProfile(
        id: 'm5',
        name: 'm5',
        hosts: const <String>['10.0.0.5'],
        port: 47821,
        fingerprint: fpM5,
        token: 'tok',
        tunnel: const TunnelInfo(url: 'wss://tunnel.example.com', accessClientId: 'id', accessClientSecret: 'secret'),
        lastGoodWasTunnel: true,
        nodes: pinnedM5().nodes,
        clusterPrimary: 'm5',
        clusterTerm: 3,
      );
      world.extra.add(clusterNode('m7', 'm7', '10.0.0.7:47821', 'd' * 64));
      final List<List<PinnedNode>> learned = <List<PinnedNode>>[];
      final BridgeClient client = BridgeClient(
        connector: (Endpoint e) => e.kind == RouteKind.tunnel ? world.connect(Endpoint(host: '10.0.0.5', port: 47821, fingerprint: fpM5)) : world.connect(e),
        profile: viaTunnel,
        onCluster: (String? p, int t, List<PinnedNode> n) => learned.add(n),
      );
      client.start();
      await until(() => client.isConnected);
      expect(client.currentStatus.route, RouteKind.tunnel);
      expect(learned, isEmpty);
      await client.dispose();
    });
  });
}
