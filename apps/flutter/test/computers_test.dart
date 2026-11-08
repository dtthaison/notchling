import 'package:agent_console/app/providers.dart';
import 'package:agent_console/core/net/bridge_client.dart';
import 'package:agent_console/core/net/ws_transport.dart';
import 'package:agent_console/core/pairing/mac_profile.dart';
import 'package:agent_console/core/pairing/mac_store.dart';
import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/chat/chat_cache.dart';
import 'package:agent_console/features/chat/chat_notifier.dart';
import 'package:agent_console/features/connection/connection.dart';
import 'package:agent_console/features/macs/computers.dart';
import 'package:agent_console/features/macs/macs_notifier.dart';
import 'package:agent_console/features/macs/session_ref.dart';
import 'package:agent_console/features/sessions/archive_store.dart';
import 'package:agent_console/features/sessions/sessions_notifier.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:flutter_test/flutter_test.dart';

import 'support/fake_ws.dart';
import 'support/harness.dart';

const MacProfile macA = MacProfile(
  id: 'aaaaaaaaaaaaaaaa',
  name: 'Mac A',
  hosts: <String>['10.0.0.1'],
  port: 47821,
  fingerprint: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  token: 'tok-a',
);

const MacProfile macB = MacProfile(
  id: 'bbbbbbbbbbbbbbbb',
  name: 'Linux B',
  hosts: <String>['10.0.0.2'],
  port: 47821,
  fingerprint: 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
  token: 'tok-b',
);

/// Two paired computers, both with a session called `s1`. Each socket answers only its own computer's host.
({ProviderContainer container, FakeWs a, FakeWs b}) twoComputers({MemorySecretStore? secrets}) {
  final FakeWs a = macWithSessions(<Map<String, Object?>>[sessionJson('s1'), sessionJson('only-a')]);
  final FakeWs b = macWithSessions(<Map<String, Object?>>[sessionJson('s1', state: 'needsInput')]);
  final MemorySecretStore store = secrets ??
      (MemorySecretStore()
        ..values['macs.v1'] = encodeProfiles(<MacProfile>[macA, macB])
        ..values['macs.active'] = macA.id);
  final ProviderContainer container = ProviderContainer(overrides: <Override>[
    secretStoreProvider.overrideWithValue(store),
    chatCacheProvider.overrideWithValue(MemoryChatCache()),
    archiveStoreProvider.overrideWithValue(MemoryArchiveStore()),
    wsConnectorProvider.overrideWithValue((Endpoint endpoint) async => endpoint.host == '10.0.0.2' ? b : a),
    networkChangesProvider.overrideWithValue(const Stream<void>.empty()),
  ]);
  addTearDown(container.dispose);
  return (container: container, a: a, b: b);
}

Future<void> loaded(ProviderContainer container) async {
  container.read(macsProvider);
  await until(() => container.read(macsProvider).loaded);
}

Future<void> connectedTo(ProviderContainer container, String host) async {
  container.listen<BridgeClient?>(computerClientProvider(host), (BridgeClient? a, BridgeClient? b) {});
  await until(() => container.read(computerClientProvider(host))?.isConnected ?? false);
}

List<Map<String, Object?>> ops(FakeWs socket, String op) => socket.sent.where((Map<String, Object?> m) => m['op'] == op).toList();

void main() {
  group('SessionRef', () {
    test('a qualified id is the host, a slash, then the session id as the computer gave it', () {
      expect(const SessionRef('aaaaaaaaaaaaaaaa', 's1').qualified, 'aaaaaaaaaaaaaaaa/s1');
      expect(const SessionRef('aaaaaaaaaaaaaaaa', 'codex:019a/x').qualified, 'aaaaaaaaaaaaaaaa/codex:019a/x');
      expect(SessionRef.parse('aaaaaaaaaaaaaaaa/codex:019a/x'), const SessionRef('aaaaaaaaaaaaaaaa', 'codex:019a/x'));
    });

    test('bare session ids never read as qualified', () {
      expect(SessionRef.parse('0f8fad5b-d9cb-469f-a165-70867728950e'), isNull);
      expect(SessionRef.parse('pty:3'), isNull);
      expect(SessionRef.parse('agy:abc'), isNull);
      expect(SessionRef.parse('AAAAAAAAAAAAAAAA/s1'), isNull, reason: 'host ids are lowercase hex');
      expect(SessionRef.parse('aaaaaaaaaaaaaaaa/'), isNull);
    });

    test('the same session id on two computers is two different refs', () {
      expect(const SessionRef('aaaaaaaaaaaaaaaa', 's1') == const SessionRef('bbbbbbbbbbbbbbbb', 's1'), isFalse);
      expect(<SessionRef>{const SessionRef('aaaaaaaaaaaaaaaa', 's1'), const SessionRef('bbbbbbbbbbbbbbbb', 's1')}, hasLength(2));
    });
  });

  group('two computers with the same session id', () {
    test('sessions are listed per computer, each id qualified by its host', () async {
      final (:ProviderContainer container, :FakeWs a, :FakeWs b) = twoComputers();
      await loaded(container);
      container.listen<List<ComputerSessions>>(allComputersSessionsProvider, (List<ComputerSessions>? x, List<ComputerSessions> y) {});
      await until(() => container.read(allComputersSessionsProvider).every((ComputerSessions c) => c.state.phase == LoadPhase.ready));
      final List<ComputerSessions> all = container.read(allComputersSessionsProvider);
      expect(all.map((ComputerSessions c) => c.computer.name), <String>['Mac A', 'Linux B']);
      expect(all[0].state.sessions.map((Session s) => s.id), <String>['aaaaaaaaaaaaaaaa/s1', 'aaaaaaaaaaaaaaaa/only-a']);
      expect(all[1].state.sessions.map((Session s) => s.id), <String>['bbbbbbbbbbbbbbbb/s1']);
      expect(all[1].state.sessions.single.state, SessionState.needsInput, reason: "B's s1 is B's, not A's");
      // The computer that isn't in use still pushes its list: it is subscribed to sessions only.
      expect(ops(b, 'subscribe').last, containsPair('sessions', true));
      expect(ops(b, 'subscribe').last.containsKey('sessionId'), isFalse);
    });

    test('a message to s1 picked on B goes to B, and one to s1 picked on A goes to A', () async {
      final (:ProviderContainer container, :FakeWs a, :FakeWs b) = twoComputers();
      await loaded(container);
      container.listen<ChatState>(chatProvider('s1'), (ChatState? x, ChatState y) {});

      container.read(selectedSessionProvider.notifier).select(const SessionRef('bbbbbbbbbbbbbbbb', 's1'));
      expect(container.read(macsProvider).activeId, macB.id, reason: 'picking a session makes its computer the one in use');
      expect(container.read(selectedSessionIdProvider), 's1');
      await connectedTo(container, macB.id);
      expect(await container.read(chatProvider('s1').notifier).send('to B'), isTrue);
      await until(() => ops(b, 'session.message').isNotEmpty);
      expect(ops(b, 'session.message').single, allOf(containsPair('sessionId', 's1'), containsPair('text', 'to B')));
      expect(ops(a, 'session.message'), isEmpty);

      container.read(selectedSessionProvider.notifier).select(const SessionRef('aaaaaaaaaaaaaaaa', 's1'));
      await connectedTo(container, macA.id);
      expect(await container.read(chatProvider('s1').notifier).send('to A'), isTrue);
      await until(() => ops(a, 'session.message').isNotEmpty);
      expect(ops(a, 'session.message').single, containsPair('text', 'to A'));
      expect(ops(b, 'session.message'), hasLength(1));
    });

    test('switching the computer in use under an open chat never moves its answers to the other computer', () async {
      final (:ProviderContainer container, :FakeWs a, :FakeWs b) = twoComputers();
      await loaded(container);
      container.read(selectedSessionProvider.notifier).select(const SessionRef('bbbbbbbbbbbbbbbb', 's1'));
      await connectedTo(container, macB.id);
      await connectedTo(container, macA.id);

      await container.read(macsProvider.notifier).setActive(macA.id);
      expect(container.read(selectedSessionIdProvider), isNull, reason: "A's s1 was never picked");

      const PendingPrompt prompt = PendingPrompt(requestId: 'r1', sessionId: 's1', isPermission: true, toolName: 'Bash', summary: 'ls', questions: <PromptQuestion>[]);
      await container.read(chatProvider('s1').notifier).allow(prompt, allow: true);
      expect(ops(a, 'prompt.answer'), isEmpty);
      expect(ops(b, 'prompt.answer').single, containsPair('requestId', 'r1'));
    });

    test('a bare pick qualifies with the computer in use', () async {
      final (:ProviderContainer container, a: _, b: _) = twoComputers();
      await loaded(container);
      container.read(selectedSessionIdProvider.notifier).select('s1');
      expect(container.read(selectedSessionProvider), const SessionRef('aaaaaaaaaaaaaaaa', 's1'));
      await container.read(macsProvider.notifier).setActive(macB.id);
      expect(container.read(selectedSessionIdProvider), isNull);
    });
  });

  group('what the phone keeps per computer', () {
    test('a phone paired before this change keeps its cache and archive keys for the Mac in use', () async {
      final (:ProviderContainer container, a: _, b: _) = twoComputers();
      await loaded(container);
      expect(container.read(macsProvider).scopeOf(macA.id), '', reason: 'the Mac in use keeps bare keys');
      expect(container.read(macsProvider).scopeOf(macB.id), macB.id);
      expect(storageKey('', 's1'), 's1');
      expect(storageKey(macB.id, 's1'), 'bbbbbbbbbbbbbbbb/s1');
    });

    test('a computer paired now gets its own scope', () async {
      final (:ProviderContainer container, a: _, b: _) = twoComputers(secrets: MemorySecretStore());
      await loaded(container);
      await container.read(macsProvider.notifier).add(macA);
      expect(container.read(macsProvider).scopeOf(macA.id), macA.id);
    });

    test('two scopes never read each other’s cached chat', () async {
      final MemoryChatCache inner = MemoryChatCache();
      final ScopedChatCache onA = ScopedChatCache(inner, '');
      final ScopedChatCache onB = ScopedChatCache(inner, macB.id);
      await onA.saveMessages('s1', const <ChatMessage>[]);
      await onB.saveMessages('s1', <ChatMessage>[ChatMessage.fromJson(<String, Object?>{'id': 'm1', 'role': 'user', 'text': 'B', 'time': 0})]);
      expect(await onA.loadMessages('s1'), isEmpty);
      expect((await onB.loadMessages('s1')).single.text, 'B');
      expect(inner.messages.keys, containsAll(<String>['s1', 'bbbbbbbbbbbbbbbb/s1']));
    });

    test('archiving s1 on one computer leaves s1 on the other in the list', () {
      final Session s1 = Session.fromJson(sessionJson('s1'));
      final Map<String, int> archived = <String, int>{storageKey(macB.id, 's1'): s1.lastActivity.millisecondsSinceEpoch};
      expect(hiddenSessionIdsIn(archived, <Session>[s1], ''), isEmpty);
      expect(hiddenSessionIdsIn(archived, <Session>[s1], macB.id), <String>{'s1'});
    });
  });
}
