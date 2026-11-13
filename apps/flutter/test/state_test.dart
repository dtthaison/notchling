import 'dart:convert';

import 'package:agent_console/app/notifications.dart';
import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/chat/chat_notifier.dart';
import 'package:agent_console/features/connection/subscription_sync.dart';
import 'package:agent_console/features/sessions/sessions_notifier.dart';
import 'package:agent_console/features/visualize/viz_providers.dart';
import 'package:agent_console/features/visualize/viz_webview.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/fake_ws.dart';
import 'support/harness.dart';

Session _session(String id, SessionState state, {int minutesAgo = 0, String name = 'S'}) => Session(
      id: id,
      name: '$name$id',
      project: 'p',
      cwd: '/p',
      entrypoint: 'cli',
      state: state,
      control: SessionControl.console,
      lastActivity: DateTime(2026, 11, 2, 12).subtract(Duration(minutes: minutesAgo)),
    );

void main() {
  group('groupSessions', () {
    final List<Session> sessions = <Session>[
      _session('a', SessionState.done, minutesAgo: 5),
      _session('b', SessionState.running, minutesAgo: 9),
      _session('c', SessionState.needsInput),
      _session('d', SessionState.running, minutesAgo: 1),
      _session('e', SessionState.exited),
    ];

    test('orders groups Cần input, Đang chạy, Xong, Đã thoát and newest first inside a group', () {
      final Map<SessionState, List<Session>> groups = groupSessions(sessions);
      expect(groups.keys.toList(), <SessionState>[SessionState.needsInput, SessionState.running, SessionState.done, SessionState.exited]);
      expect(groups[SessionState.running]!.map((Session s) => s.id), <String>['d', 'b']);
    });

    test('empty groups are left out', () {
      expect(groupSessions(<Session>[_session('a', SessionState.done)]).keys, <SessionState>[SessionState.done]);
      expect(groupSessions(<Session>[]), isEmpty);
    });

    test('search matches name, project and subagent text, case-insensitively', () {
      final Session withAgent = Session(
        id: 'x',
        name: 'Alpha',
        project: 'maneki',
        cwd: '/m',
        entrypoint: 'cli',
        state: SessionState.running,
        control: SessionControl.console,
        lastActivity: DateTime(2026),
        subagents: <Subagent>[Subagent(id: '1', type: 'Explore', description: 'Scan the repo', model: 'haiku', finished: false, running: true, lastActivity: DateTime(2026))],
      );
      final List<Session> all = <Session>[...sessions, withAgent];
      expect(groupSessions(all, query: 'ALPHA').values.expand((List<Session> g) => g).single.id, 'x');
      expect(groupSessions(all, query: 'maneki').values.expand((List<Session> g) => g).single.id, 'x');
      expect(groupSessions(all, query: 'scan the').values.expand((List<Session> g) => g).single.id, 'x');
      expect(groupSessions(all, query: 'nothing matches'), isEmpty);
    });
  });

  group('sessions and chat over a fake Mac', () {
    test('loads the list on connect and applies pushed updates', () async {
      final FakeWs socket = macWithSessions(<Map<String, Object?>>[sessionJson('s1'), sessionJson('s2', state: 'needsInput')]);
      final ProviderContainer container = containerWith(socket);
      await connect(container);
      container.listen<SessionsState>(sessionsProvider, (SessionsState? a, SessionsState b) {});
      await until(() => container.read(sessionsProvider).phase == LoadPhase.ready);
      expect(container.read(sessionsProvider).sessions.map((Session s) => s.id), <String>['s1', 's2']);

      socket.event('sessions', <String, Object?>{'data': <Object?>[sessionJson('s3')]});
      await until(() => container.read(sessionsProvider).sessions.length == 1);
      expect(container.read(sessionsProvider).sessions.single.id, 's3');
    });

    test('a failed list load shows the error state, and refresh retries', () async {
      int calls = 0;
      final FakeWs socket = FakeWs(onSend: (FakeWs s, Map<String, Object?> m) {
        if (m['op'] == 'hello') {
          s.reply(m['id']!);
        } else if (m['op'] == 'sessions.list') {
          calls++;
          if (calls == 1) {
            s.fail(m['id']!, 'internal', 'boom');
          } else {
            s.reply(m['id']!, data: <String, Object?>{'sessions': <Object?>[sessionJson('ok')]});
          }
        }
      });
      final ProviderContainer container = containerWith(socket);
      await connect(container);
      container.listen<SessionsState>(sessionsProvider, (SessionsState? a, SessionsState b) {});
      await until(() => container.read(sessionsProvider).phase == LoadPhase.error);
      await container.read(sessionsProvider.notifier).refresh();
      expect(container.read(sessionsProvider).phase, LoadPhase.ready);
      expect(container.read(sessionsProvider).sessions.single.id, 'ok');
    });

    test('a message goes to exactly the session the chat was opened for', () async {
      final FakeWs socket = macWithSessions(<Map<String, Object?>>[sessionJson('s1'), sessionJson('s2')]);
      final ProviderContainer container = containerWith(socket);
      await connect(container);
      container.listen<SessionsState>(sessionsProvider, (SessionsState? a, SessionsState b) {});
      container.listen<ChatState>(chatProvider('s2'), (ChatState? a, ChatState b) {});
      container.listen<ChatState>(chatProvider('s1'), (ChatState? a, ChatState b) {});
      await until(() => container.read(sessionsProvider).sessions.length == 2);

      await container.read(chatProvider('s2').notifier).send('  hello there  ');

      final List<Map<String, Object?>> sends = socket.sent.where((Map<String, Object?> m) => m['op'] == 'session.message').toList();
      expect(sends, hasLength(1));
      expect(sends.single['sessionId'], 's2');
      expect(sends.single['text'], 'hello there', reason: 'trimmed');
      expect(container.read(chatProvider('s2')).send, isA<SendIdle>());
    });

    test('a view-only session is refused locally: nothing is sent', () async {
      final FakeWs socket = macWithSessions(<Map<String, Object?>>[sessionJson('v', control: 'viewOnly')]);
      final ProviderContainer container = containerWith(socket);
      await connect(container);
      container.listen<SessionsState>(sessionsProvider, (SessionsState? a, SessionsState b) {});
      container.listen<ChatState>(chatProvider('v'), (ChatState? a, ChatState b) {});
      await until(() => container.read(sessionsProvider).sessions.isNotEmpty);

      await container.read(chatProvider('v').notifier).send('hi');

      final SendState result = container.read(chatProvider('v')).send;
      expect(result, isA<SendFailed>().having((SendFailed f) => f.viewOnly, 'viewOnly', isTrue));
      expect(socket.sent.where((Map<String, Object?> m) => m['op'] == 'session.message'), isEmpty);
    });

    test('a view_only answer from the Mac is surfaced the same way, and the text is kept for retry', () async {
      final FakeWs socket = happyMac(auto: <String, Map<String, Object?>>{
        'sessions.list': <String, Object?>{'sessions': <Object?>[sessionJson('p')]},
        'chat.get': <String, Object?>{'messages': <Object?>[], 'hasMore': false},
      });
      final FakeWs wrapped = FakeWs(onSend: (FakeWs s, Map<String, Object?> m) {
        if (m['op'] == 'session.message') {
          s.fail(m['id']!, 'view_only', 'chỉ xem');
        } else {
          socket.onSend!(s, m);
        }
      });
      final ProviderContainer container = containerWith(wrapped);
      await connect(container);
      container.listen<SessionsState>(sessionsProvider, (SessionsState? a, SessionsState b) {});
      container.listen<ChatState>(chatProvider('p'), (ChatState? a, ChatState b) {});
      await until(() => container.read(sessionsProvider).sessions.isNotEmpty);

      expect(await container.read(chatProvider('p').notifier).send('keep me'), isTrue, reason: 'it is shown at once and delivered in the background');
      await until(() => container.read(chatProvider('p')).send is SendFailed);
      expect(container.read(chatProvider('p')).pending.single.failed, isTrue);
      final SendFailed failed = container.read(chatProvider('p')).send as SendFailed;
      expect(failed.viewOnly, isTrue);
      expect(failed.text, 'keep me');
    });

    test('chat loads the first page, appends pushed messages once, and pages older ones with before', () async {
      Map<String, Object?> message(String id, String text) => <String, Object?>{'id': id, 'role': 'user', 'text': text, 'time': 1000.0};
      final FakeWs socket = FakeWs(onSend: (FakeWs s, Map<String, Object?> m) {
        switch (m['op']) {
          case 'hello':
            s.reply(m['id']!);
          case 'sessions.list':
            s.reply(m['id']!, data: <String, Object?>{'sessions': <Object?>[sessionJson('c')]});
          case 'chat.get':
            if (m.containsKey('before')) {
              s.reply(m['id']!, data: <String, Object?>{'messages': <Object?>[message('m0', 'older')], 'hasMore': false});
            } else {
              s.reply(m['id']!, data: <String, Object?>{'messages': <Object?>[message('m1', 'one'), message('m2', 'two')], 'hasMore': true});
            }
        }
      });
      final ProviderContainer container = containerWith(socket);
      await connect(container);
      container.listen<ChatState>(chatProvider('c'), (ChatState? a, ChatState b) {});
      await until(() => container.read(chatProvider('c')).phase == LoadPhase.ready);
      expect(container.read(chatProvider('c')).messages.map((ChatMessage m) => m.text), <String>['one', 'two']);
      expect(container.read(chatProvider('c')).hasMore, isTrue);

      socket.event('chat', <String, Object?>{'sessionId': 'c', 'messages': <Object?>[message('m2', 'two'), message('m3', 'three')]});
      socket.event('chat', <String, Object?>{'sessionId': 'other', 'messages': <Object?>[message('x', 'not mine')]});
      await until(() => container.read(chatProvider('c')).messages.length == 3);
      expect(container.read(chatProvider('c')).messages.map((ChatMessage m) => m.id), <String>['m1', 'm2', 'm3']);

      await container.read(chatProvider('c').notifier).loadOlder();
      expect(socket.sent.last, containsPair('before', 'm1'));
      expect(container.read(chatProvider('c')).messages.map((ChatMessage m) => m.id), <String>['m0', 'm1', 'm2', 'm3']);
      expect(container.read(chatProvider('c')).hasMore, isFalse);
    });

    test('a Console terminal without a transcript yet opens as an empty chat, not an error', () async {
      final FakeWs socket = FakeWs(onSend: (FakeWs s, Map<String, Object?> m) {
        switch (m['op']) {
          case 'hello':
            s.reply(m['id']!);
          case 'sessions.list':
            s.reply(m['id']!, data: <String, Object?>{'sessions': <Object?>[]});
          case 'chat.get':
            s.fail(m['id']!, 'not_found', 'no transcript');
        }
      });
      final ProviderContainer container = containerWith(socket);
      await connect(container);
      container.listen<ChatState>(chatProvider('pty:abc'), (ChatState? a, ChatState b) {});
      container.listen<ChatState>(chatProvider('real'), (ChatState? a, ChatState b) {});
      await until(() => container.read(chatProvider('pty:abc')).phase == LoadPhase.ready);
      await until(() => container.read(chatProvider('real')).phase == LoadPhase.error);
    });

    test('prompt answers go out as allow or as the picked option numbers', () async {
      final FakeWs socket = macWithSessions(<Map<String, Object?>>[sessionJson('s1')]);
      final ProviderContainer container = containerWith(socket);
      await connect(container);
      container.listen<ChatState>(chatProvider('s1'), (ChatState? a, ChatState b) {});
      const PendingPrompt permission = PendingPrompt(requestId: 'r1', sessionId: 's1', isPermission: true, toolName: 'Bash', summary: 'ls', questions: <PromptQuestion>[]);
      const PendingPrompt question = PendingPrompt(
          requestId: 'r2',
          sessionId: 's1',
          isPermission: false,
          toolName: 'AskUserQuestion',
          summary: 'Q',
          questions: <PromptQuestion>[PromptQuestion(header: '', question: 'Q?', options: <String>['a', 'b'], multiSelect: false)]);

      await container.read(chatProvider('s1').notifier).allow(permission, allow: true);
      await container.read(chatProvider('s1').notifier).choose(question, <int>[2]);
      final List<Map<String, Object?>> answers = socket.sent.where((Map<String, Object?> m) => m['op'] == 'prompt.answer').toList();
      expect(answers[0], allOf(containsPair('requestId', 'r1'), containsPair('allow', true)));
      expect(answers[1], allOf(containsPair('requestId', 'r2'), containsPair('choice', 2)));
    });

    test('the subscription follows the selection and the Phone tab', () async {
      final FakeWs socket = macWithSessions(<Map<String, Object?>>[sessionJson('s1')]);
      final ProviderContainer container = containerWith(socket);
      await connect(container);
      container.listen<void>(subscriptionSyncProvider, (void a, void b) {});
      container.read(selectedSessionIdProvider.notifier).select('s1');
      await until(() => socket.sent.any((Map<String, Object?> m) => m['op'] == 'subscribe' && m['sessionId'] == 's1'));

      container.read(phoneWantedProvider.notifier).set(true);
      await until(() => socket.sent.any((Map<String, Object?> m) => m['op'] == 'subscribe' && m.containsKey('phone')));
      container.read(phoneWantedProvider.notifier).set(false);
      await until(() => socket.sent.any((Map<String, Object?> m) => m['op'] == 'unsubscribe'));
    });

    test('a phone-stream error from the Mac is kept for the Phone tab', () async {
      final FakeWs socket = FakeWs(onSend: (FakeWs s, Map<String, Object?> m) {
        if (m['op'] == 'hello') {
          s.reply(m['id']!);
        } else if (m['op'] == 'subscribe' && m.containsKey('phone')) {
          s.fail(m['id']!, 'internal', 'Chưa có quyền Ghi màn hình');
        } else {
          s.reply(m['id']!);
        }
      });
      final ProviderContainer container = containerWith(socket);
      await connect(container);
      container.listen<void>(subscriptionSyncProvider, (void a, void b) {});
      container.read(selectedSessionIdProvider.notifier).select('s1');
      container.read(phoneWantedProvider.notifier).set(true);
      await until(() => container.read(phoneErrorProvider) != null);
      expect(container.read(phoneErrorProvider), contains('quyền'));
    });
  });

  group('viz', () {
    test('the suggestion is read from viz.get and a user choice is kept per session', () async {
      final FakeWs socket = happyMac(auto: <String, Map<String, Object?>>{
        'viz.get': <String, Object?>{'suggested': 'web', 'webURL': 'http://localhost:3000', 'flow': <String, Object?>{'kind': 'flow'}},
      });
      final ProviderContainer container = containerWith(socket);
      await connect(container);
      container.listen<AsyncValue<Map<String, Object?>>>(vizProvider('s1'), (AsyncValue<Map<String, Object?>>? a, AsyncValue<Map<String, Object?>> b) {});
      await until(() => container.read(vizProvider('s1')).hasValue);
      expect(VizTab.byName(container.read(vizProvider('s1')).value!['suggested']! as String), VizTab.web);

      container.read(vizTabChoiceProvider.notifier).choose('s1', VizTab.chart);
      expect(container.read(vizTabChoiceProvider)['s1'], VizTab.chart);
      expect(container.read(vizTabChoiceProvider)['s2'], isNull);
    });

    test('the spec reaches JavaScript as one safely quoted string, whatever it contains', () {
      const String nasty = "'); alert(1); //\u2028\\\"</script>\n";
      final Map<String, Object?> spec = <String, Object?>{'kind': 'flow', 'title': nasty};
      final String script = renderScript(spec);
      expect(script, startsWith('window.renderViz(JSON.parse('));
      expect(script, endsWith('));'));
      expect(script.contains('\n'), isFalse, reason: 'newlines are escaped inside the literal');
      final String literal = script.substring('window.renderViz(JSON.parse('.length, script.length - '));'.length);
      expect(jsonDecode(jsonDecode(literal) as String), spec);
    });
  });

  group('notifications', () {
    test('detects sessions that newly need input', () {
      final List<Session> before = <Session>[_session('a', SessionState.running), _session('b', SessionState.needsInput)];
      final List<Session> after = <Session>[_session('a', SessionState.needsInput), _session('b', SessionState.needsInput), _session('c', SessionState.needsInput)];
      expect(newlyNeedingInput(before, after), <String>{'a', 'c'});
      expect(newlyNeedingInput(after, after), isEmpty);
    });

    test('fires only in the background, connected, and after a first list', () {
      expect(shouldNotify(hadPreviousList: true, lifecycle: AppLifecycleState.paused, connected: true), isTrue);
      expect(shouldNotify(hadPreviousList: true, lifecycle: AppLifecycleState.inactive, connected: true), isTrue);
      expect(shouldNotify(hadPreviousList: true, lifecycle: AppLifecycleState.resumed, connected: true), isFalse);
      expect(shouldNotify(hadPreviousList: true, lifecycle: AppLifecycleState.paused, connected: false), isFalse);
      expect(shouldNotify(hadPreviousList: false, lifecycle: AppLifecycleState.paused, connected: true), isFalse);
    });
  });
}
