import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/chat/chat_parts.dart';
import 'package:agent_console/features/chat/send_now.dart';
import 'package:agent_console/features/sessions/sessions_notifier.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/fake_ws.dart';
import 'support/harness.dart';

void main() {
  final ChatMessage queued = ChatMessage(id: 'q', role: ChatRole.user, text: 'next please', time: DateTime(2026), queued: true);

  Future<(ProviderContainer, FakeWs)> open(WidgetTester tester, {String control = 'console', String sendNowReply = 'ok'}) async {
    final FakeWs socket = FakeWs(onSend: (FakeWs s, Map<String, Object?> m) {
      switch (m['op']) {
        case 'hello':
          s.reply(m['id']!);
        case 'sessions.list':
          s.reply(m['id']!, data: <String, Object?>{'sessions': <Object?>[sessionJson('s', control: control)]});
        case 'subscribe':
          s.reply(m['id']!, data: <String, Object?>{'subscribed': true});
        case 'session.sendNow':
          if (sendNowReply == 'ok') {
            s.reply(m['id']!, data: <String, Object?>{'sent': true});
          } else {
            s.fail(
              m['id']!,
              'invalid',
              sendNowReply == 'invalidEnglish' ? 'No such op "session.sendNow".' : 'Không có lệnh "session.sendNow".',
            );
          }
      }
    });
    final ProviderContainer container = containerWith(socket);
    await tester.runAsync(() async {
      await connect(container);
      container.listen<SessionsState>(sessionsProvider, (SessionsState? a, SessionsState b) {});
      await until(() => container.read(sessionsProvider).sessions.isNotEmpty);
    });
    await tester.pumpWidget(UncontrolledProviderScope(
      container: container,
      child: host(UserBubble(message: queued, sessionId: 's')),
    ));
    return (container, socket);
  }

  testWidgets('a queued bubble offers Send now; confirming sends session.sendNow', (WidgetTester tester) async {
    final (ProviderContainer, FakeWs) rig = await open(tester);
    expect(find.text('Send now'), findsOneWidget);
    await tester.tap(find.text('Send now'));
    await tester.pumpAndSettle();
    expect(find.text('Interrupt the running turn to send now?'), findsOneWidget);
    await tester.tap(find.text('Cancel').evaluate().isNotEmpty ? find.text('Cancel') : find.byType(TextButton).first);
    await tester.pumpAndSettle();
    expect(rig.$2.sent.where((Map<String, Object?> m) => m['op'] == 'session.sendNow'), isEmpty, reason: 'cancel sends nothing');

    await tester.tap(find.text('Send now'));
    await tester.pumpAndSettle();
    await tester.tap(find.descendant(of: find.byType(AlertDialog), matching: find.text('Send now')));
    await tester.runAsync(() => until(() => rig.$2.sent.any((Map<String, Object?> m) => m['op'] == 'session.sendNow')));
    await tester.pump();
    expect(find.byType(CircularProgressIndicator), findsOneWidget, reason: 'spinner while waiting for the transcript');
    await tester.pumpWidget(const SizedBox());
    await tester.pump(const Duration(seconds: 9));
  });

  testWidgets('hidden on a view-only session', (WidgetTester tester) async {
    await open(tester, control: 'viewOnly');
    expect(find.text('Send now'), findsNothing);
    expect(find.text('waiting'), findsOneWidget);
  });

  testWidgets('an older Mac (unknown op) hides the action from then on', (WidgetTester tester) async {
    final (ProviderContainer, FakeWs) rig = await open(tester, sendNowReply: 'invalid');
    await tester.tap(find.text('Send now'));
    await tester.pumpAndSettle();
    await tester.tap(find.descendant(of: find.byType(AlertDialog), matching: find.text('Send now')));
    await tester.runAsync(() => until(() => !rig.$1.read(sendNowSupportProvider)));
    await tester.pumpAndSettle();
    expect(find.text('Send now'), findsNothing);
  });

  testWidgets('a Mac answering in English (No such op) hides the action too', (WidgetTester tester) async {
    final (ProviderContainer, FakeWs) rig = await open(tester, sendNowReply: 'invalidEnglish');
    await tester.tap(find.text('Send now'));
    await tester.pumpAndSettle();
    await tester.tap(find.descendant(of: find.byType(AlertDialog), matching: find.text('Send now')));
    await tester.runAsync(() => until(() => !rig.$1.read(sendNowSupportProvider)));
    await tester.pumpAndSettle();
    expect(find.text('Send now'), findsNothing);
  });
}
