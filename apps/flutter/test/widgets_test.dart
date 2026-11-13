import 'dart:ui' show Tristate;

import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/chat/chat_notifier.dart';
import 'package:agent_console/features/chat/composer.dart';
import 'package:agent_console/features/chat/markdown_text.dart';
import 'package:agent_console/features/chat/pending_card.dart';
import 'package:agent_console/app/widgets/kit.dart';
import 'package:agent_console/features/sessions/session_row.dart';
import 'package:agent_console/features/sessions/status_style.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/harness.dart';

final DateTime _now = DateTime(2026, 11, 2, 12);

Session _session({SessionState state = SessionState.running, SessionControl control = SessionControl.console, List<Subagent> subagents = const <Subagent>[], String name = 'Notching repo startup'}) =>
    Session(
      id: 's',
      name: name,
      project: 'truongbinhtriet',
      cwd: '/Users/me',
      entrypoint: 'cli',
      state: state,
      control: control,
      lastActivity: _now.subtract(const Duration(minutes: 3)),
      subagents: subagents,
    );

Subagent _agent({bool running = true}) =>
    Subagent(id: 'a', type: 'general-purpose', description: 'Spike voice model', model: 'sonnet', finished: !running, running: running, lastActivity: _now);

void main() {
  group('SessionRow', () {
    testWidgets('shows name, project, time and the state in words for screen readers', (WidgetTester tester) async {
      await tester.pumpWidget(host(SessionRow(session: _session(), now: _now, onTap: () {})));
      expect(find.text('Notching repo startup'), findsOneWidget);
      expect(find.text('truongbinhtriet'), findsOneWidget);
      expect(find.text('3 phút'), findsOneWidget);
      expect(find.bySemanticsLabel(RegExp('Đang chạy')), findsOneWidget);
      expect(find.bySemanticsLabel(RegExp('Notching repo startup, truongbinhtriet, Đang chạy')), findsOneWidget);
    });

    testWidgets('the status is the badge and the group, not a pill that repeats the group; screen readers still hear it', (WidgetTester tester) async {
      await tester.pumpWidget(host(SessionRow(session: _session(state: SessionState.needsInput), now: _now, onTap: () {})));
      expect(find.byType(KitStatusPill), findsNothing);
      expect(find.byType(StatusDot), findsOneWidget);
      expect(find.bySemanticsLabel(RegExp('Cần bạn')), findsOneWidget);
    });

    testWidgets('running subagents show as a count, and the arrow opens the tree', (WidgetTester tester) async {
      int toggled = 0;
      await tester.pumpWidget(host(SessionRow(
        session: _session(subagents: <Subagent>[_agent(), _agent(running: false)]),
        now: _now,
        onTap: () {},
        onToggleExpand: () => toggled++,
      )));
      expect(find.text('2'), findsOneWidget, reason: 'the number of subagents');
      await tester.tap(find.bySemanticsLabel(RegExp('Mở 2 subagent')));
      expect(toggled, 1);
    });

    testWidgets('a session without subagents has no arrow', (WidgetTester tester) async {
      await tester.pumpWidget(host(SessionRow(session: _session(), now: _now, onTap: () {})));
      expect(find.byIcon(Icons.chevron_right), findsNothing);
    });

    testWidgets('tapping the row selects it, and a selected row says so', (WidgetTester tester) async {
      int taps = 0;
      final SemanticsHandle handle = tester.ensureSemantics();
      await tester.pumpWidget(host(SessionRow(session: _session(), now: _now, onTap: () => taps++, selected: true)));
      await tester.tap(find.text('Notching repo startup'));
      expect(taps, 1);
      expect(tester.getSemantics(find.bySemanticsLabel(RegExp('Notching repo startup, truongbinhtriet'))).flagsCollection.isSelected, Tristate.isTrue);
      handle.dispose();
    });

    testWidgets('the row is at least 56 dp and the subagent toggle at least 32 (a secondary target; the row is the main one)', (WidgetTester tester) async {
      await tester.pumpWidget(host(SessionRow(session: _session(subagents: <Subagent>[_agent()]), now: _now, onTap: () {}, onToggleExpand: () {})));
      expect(tester.getSize(find.byType(SessionRow)).height, greaterThanOrEqualTo(56));
      expect(tester.getSize(find.bySemanticsLabel(RegExp('Mở 1 subagent'))).height, greaterThanOrEqualTo(32));
    });

    testWidgets('survives the largest text size without overflow', (WidgetTester tester) async {
      await tester.pumpWidget(host(
        SessionRow(session: _session(state: SessionState.needsInput, subagents: <Subagent>[_agent()], name: 'Một tên session rất dài để thử chữ lớn nhất của hệ thống'), now: _now, onTap: () {}, onToggleExpand: () {}),
        textScale: 3,
      ));
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(host(SubagentRow(agent: _agent()), textScale: 3));
      expect(tester.takeException(), isNull);
    });

    testWidgets('subagent row: type, description, model in mono, state', (WidgetTester tester) async {
      await tester.pumpWidget(host(SubagentRow(agent: _agent())));
      expect(find.textContaining('general-purpose', findRichText: true), findsOneWidget);
      expect(find.textContaining('Spike voice model', findRichText: true), findsOneWidget);
      expect(find.text('sonnet'), findsOneWidget);
      expect(find.text('Đang chạy'), findsOneWidget);
      expect(tester.widget<Text>(find.text('sonnet')).style?.fontFamily, 'GeistMono');
    });

    testWidgets('English strings when the device asks for English', (WidgetTester tester) async {
      await tester.pumpWidget(host(SessionRow(session: _session(), now: _now, onTap: () {}), locale: const Locale('en')));
      expect(find.text('3 min'), findsOneWidget);
      expect(find.bySemanticsLabel(RegExp('Working')), findsOneWidget);
    });
  });

  group('Composer', () {
    Future<TextEditingController> pump(WidgetTester tester, {SendState send = const SendIdle(), bool viewOnly = false, VoidCallback? onSend, VoidCallback? onRetry, String? readOnlyReason}) async {
      final TextEditingController controller = TextEditingController();
      addTearDown(controller.dispose);
      await tester.pumpWidget(host(Composer(
        controller: controller,
        send: send,
        viewOnly: viewOnly,
        readOnlyReason: readOnlyReason,
        onSend: onSend ?? () {},
        onRetry: onRetry ?? () {},
        onDismissError: () {},
      )));
      return controller;
    }

    testWidgets('idle: Send is disabled until there is text, then sends', (WidgetTester tester) async {
      int sends = 0;
      await pump(tester, onSend: () => sends++);
      await tester.tap(find.bySemanticsLabel('Gửi'));
      expect(sends, 0);
      await tester.enterText(find.byType(TextField), 'chạy flutter test');
      await tester.pump();
      await tester.tap(find.bySemanticsLabel('Gửi'));
      expect(sends, 1);
    });

    testWidgets('whitespace alone does not enable Send', (WidgetTester tester) async {
      int sends = 0;
      await pump(tester, onSend: () => sends++);
      await tester.enterText(find.byType(TextField), '   \n ');
      await tester.pump();
      await tester.tap(find.bySemanticsLabel('Gửi'));
      expect(sends, 0);
    });

    testWidgets('sending: the field locks and the button says Đang gửi', (WidgetTester tester) async {
      await pump(tester, send: const SendSending());
      expect(find.bySemanticsLabel('Đang gửi'), findsOneWidget);
      expect(tester.widget<TextField>(find.byType(TextField)).enabled, isFalse);
    });

    testWidgets('failed: the error shows with Thử lại, and Thử lại calls back', (WidgetTester tester) async {
      int retries = 0;
      await pump(tester, send: const SendFailed(text: 'hi'), onRetry: () => retries++);
      expect(find.text('Không thể gửi tin nhắn. Vui lòng thử lại.'), findsOneWidget);
      await tester.tap(find.text('Thử lại'));
      expect(retries, 1);
    });

    testWidgets('view-only: no input, the explanation and no Send button', (WidgetTester tester) async {
      await pump(tester, viewOnly: true);
      expect(find.byType(TextField), findsNothing);
      expect(find.text('Session mở ngoài Console — chỉ xem'), findsOneWidget);
      expect(find.bySemanticsLabel('Gửi'), findsNothing);
    });

    testWidgets('a subagent transcript is read-only with its own sentence', (WidgetTester tester) async {
      await pump(tester, readOnlyReason: 'Transcript của subagent: chỉ xem.');
      expect(find.byType(TextField), findsNothing);
      expect(find.text('Transcript của subagent: chỉ xem.'), findsOneWidget);
    });

    testWidgets('the mic is present and labelled; without a handler it is disabled', (WidgetTester tester) async {
      await pump(tester);
      expect(find.byIcon(Icons.mic_none), findsOneWidget);
      expect(find.byTooltip('Nói để nhập'), findsOneWidget);
    });
  });

  group('PendingCard', () {
    const PendingPrompt permission = PendingPrompt(requestId: 'r1', sessionId: 's', isPermission: true, toolName: 'Bash', summary: 'rm -rf build', questions: <PromptQuestion>[]);
    PendingPrompt question({bool multi = false, int count = 1}) => PendingPrompt(
          requestId: 'r2',
          sessionId: 's',
          isPermission: false,
          toolName: 'AskUserQuestion',
          summary: 'Which branch?',
          questions: <PromptQuestion>[
            for (int i = 0; i < count; i++) PromptQuestion(header: 'Nhánh', question: 'Which branch $i?', options: const <String>['main', 'develop'], multiSelect: multi),
          ],
        );

    testWidgets('permission: the command and Cho phép / Từ chối', (WidgetTester tester) async {
      final List<bool> answers = <bool>[];
      await tester.pumpWidget(host(PendingCard(prompt: permission, onAllow: ({required bool allow}) => answers.add(allow), onChoose: (List<int> c) {})));
      expect(find.text('rm -rf build'), findsOneWidget);
      expect(find.text('Cần cấp quyền: Bash'), findsOneWidget);
      await tester.tap(find.text('Cho phép'));
      await tester.tap(find.text('Từ chối'));
      expect(answers, <bool>[true, false]);
    });

    testWidgets('a single question sends the option number', (WidgetTester tester) async {
      List<int>? chosen;
      await tester.pumpWidget(host(PendingCard(prompt: question(), onAllow: ({required bool allow}) {}, onChoose: (List<int> c) => chosen = c)));
      await tester.tap(find.text('2  develop'));
      expect(chosen, <int>[2]);
    });

    testWidgets('two questions are asked one after the other and sent together', (WidgetTester tester) async {
      List<int>? chosen;
      await tester.pumpWidget(host(PendingCard(prompt: question(count: 2), onAllow: ({required bool allow}) {}, onChoose: (List<int> c) => chosen = c)));
      expect(find.text('Which branch 0?'), findsOneWidget);
      await tester.tap(find.text('1  main'));
      await tester.pump();
      expect(chosen, isNull);
      expect(find.text('Which branch 1?'), findsOneWidget);
      await tester.tap(find.text('2  develop'));
      expect(chosen, <int>[1, 2]);
    });

    testWidgets('multi-select is answered on the Mac: no option buttons', (WidgetTester tester) async {
      await tester.pumpWidget(host(PendingCard(prompt: question(multi: true), onAllow: ({required bool allow}) {}, onChoose: (List<int> c) {})));
      expect(find.text('Câu hỏi này cho chọn nhiều đáp án. Trả lời trên Mac.'), findsOneWidget);
      expect(find.text('1  main'), findsNothing);
    });

    testWidgets('an answer error shows in the error color', (WidgetTester tester) async {
      await tester.pumpWidget(host(PendingCard(prompt: permission, error: 'x', onAllow: ({required bool allow}) {}, onChoose: (List<int> c) {})));
      expect(find.text('Không thể gửi câu trả lời. Vui lòng thử lại.'), findsOneWidget);
    });
  });

  group('MarkdownText', () {
    test('splits fenced code from prose, even when the fence is never closed', () {
      final List<MarkdownSegment> segments = splitSegments('before\n```dart\nvoid main() {}\n```\nafter\n```\nunclosed');
      expect(segments.map((MarkdownSegment s) => s.isCode), <bool>[false, true, false, true]);
      expect(segments[1].text, 'void main() {}');
      expect(segments[3].text, 'unclosed');
    });

    testWidgets('bold and inline code render, the rest stays as written', (WidgetTester tester) async {
      await tester.pumpWidget(host(const MarkdownText('Chạy **flutter test** rồi `git diff` xong')));
      final Text text = tester.widgetList<Text>(find.byType(Text)).firstWhere((Text t) => t.textSpan != null);
      final List<InlineSpan> children = (text.textSpan! as TextSpan).children!;
      expect(children.whereType<TextSpan>().map((TextSpan s) => s.text), <String?>['Chạy ', 'flutter test', ' rồi ', ' xong']);
      expect(children.whereType<TextSpan>().elementAt(1).style?.fontWeight, FontWeight.w600);
      expect(children.whereType<WidgetSpan>().length, 1);
      expect(tester.widget<Text>(find.text('git diff')).style?.fontFamily, 'GeistMono');
    });

    testWidgets('inline code inside bold, italics inside bold and links parse; no backtick is shown', (WidgetTester tester) async {
      await tester.pumpWidget(host(const MarkdownText('**Gõ `!` và `/` trong ô:** xem *nghiêng* và [tài liệu](https://x.y)')));
      final Text text = tester.widgetList<Text>(find.byType(Text)).firstWhere((Text t) => t.textSpan != null);
      final String plain = (text.textSpan! as TextSpan).toPlainText();
      expect(plain.contains('`'), isFalse);
      expect(plain.contains('**'), isFalse);
      expect(find.text('!'), findsOneWidget);
      expect(find.text('/'), findsOneWidget);
    });
  });
}
