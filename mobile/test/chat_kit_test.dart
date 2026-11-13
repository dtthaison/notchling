import 'package:agent_console/app/theme/tokens.dart';
import 'package:agent_console/app/widgets/common.dart';
import 'package:agent_console/features/chat/chat_header.dart';
import 'package:agent_console/app/widgets/kit.dart';
import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/chat/chat_notifier.dart';
import 'package:agent_console/features/chat/chat_parts.dart';
import 'package:agent_console/features/chat/code_highlight.dart';
import 'package:agent_console/features/chat/composer.dart';
import 'package:agent_console/features/chat/markdown_text.dart';
import 'package:agent_console/features/chat/conversation_view.dart';
import 'package:agent_console/features/chat/shell_command.dart';
import 'package:agent_console/features/chat/slash_commands.dart';
import 'package:agent_console/features/sessions/sessions_notifier.dart';
import 'package:agent_console/features/sessions/sessions_screen.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/harness.dart';
import 'support/preview_data.dart';

void main() {
  meterTests();
  group('shell commands (same rules as the Mac)', () {
    test('a leading ! marks a command, spaces before it allowed', () {
      expect(ShellCommand.isCommand('!ls'), isTrue);
      expect(ShellCommand.isCommand('  !ls'), isTrue);
      expect(ShellCommand.isCommand('run ls!'), isFalse);
    });

    test('a command wrapped by a terminal is put back together, and twice changes nothing', () {
      const String wrapped = '!git log --oneline\n  -5 --author triet';
      final String once = ShellCommand.normalize(wrapped);
      expect(once, '!git log --oneline-5 --author triet', reason: 'wrapped in the middle of a word keeps no space, as on the Mac');
      expect(ShellCommand.normalize(once), once);
    });

    test('a pasted prompt and typographic quotes are cleaned', () {
      expect(ShellCommand.normalize('!\$ echo “hi”'), '!echo "hi"');
    });

    test('text that is not a command comes back as it was', () {
      expect(ShellCommand.normalize('hello\n  world'), 'hello\n  world');
    });
  });

  group('slash commands', () {
    test('prefix matches come first, then the rest, capped', () {
      final List<SlashCommand> found = matchSlashCommands('c', builtInSlashCommands, limit: 4);
      expect(found.length, 4);
      expect(found.first.name, 'clear');
      expect(found.every((SlashCommand c) => c.name.contains('c') || c.description.toLowerCase().contains('c')), isTrue);
      expect(matchSlashCommands('zzz', builtInSlashCommands), isEmpty);
    });
  });

  group('code highlighting', () {
    test('keywords, strings, numbers, comments and types get their own colors', () {
      final Kit kit = Kit.dark;
      final List<TextSpan> spans = highlightCode('final x = "a"; // note\nreturn 42 + Foo', kit, language: 'dart');
      Color? colorOf(String text) => spans.firstWhere((TextSpan s) => s.text == text).style?.color;
      expect(colorOf('final'), kit.syntaxKeyword);
      expect(colorOf('"a"'), kit.syntaxString);
      expect(colorOf('// note'), kit.textFaint);
      expect(colorOf('42'), kit.syntaxNumber);
      expect(colorOf('Foo'), kit.syntaxType);
      expect(spans.map((TextSpan s) => s.text).join(), 'final x = "a"; // note\nreturn 42 + Foo', reason: 'no character is lost');
    });

    test('the language comes from a fence word or a path', () {
      expect(languageOf('main.dart'), 'dart');
      expect(languageOf('swift'), 'swift');
      expect(languageOf(null), isNull);
    });
  });

  group('messages', () {
    test('consecutive tool messages fold into one group; others stay single', () {
      final List<ChatEntry> entries = groupEntries(previewConversation());
      final List<ToolGroupEntry> groups = entries.whereType<ToolGroupEntry>().toList();
      expect(groups.length, 3);
      expect(groups.first.steps.length, 4);
      expect(entries.whereType<MessageEntry>().length, previewConversation().where((ChatMessage m) => m.role != ChatRole.tools).length);
    });

    test('steps, diff, kind and facts survive a JSON round trip; older Macs without them still parse', () {
      final ChatMessage edit = previewConversation().firstWhere((ChatMessage m) => m.steps.any((ToolStep s) => s.isEdit));
      final ChatMessage again = ChatMessage.fromJson(edit.toJson());
      expect(again.steps.single.diff.map((DiffLine l) => l.kind), edit.steps.single.diff.map((DiffLine l) => l.kind));
      expect(again.steps.single.diff[1].text, edit.steps.single.diff[1].text);
      final ChatMessage shell = ChatMessage.fromJson(previewConversation().firstWhere((ChatMessage m) => m.kind == ChatKind.shell).toJson());
      expect(shell.kind, ChatKind.shell);
      expect(shell.output, contains('auth_guard.dart'));
      final ChatMessage old = ChatMessage.fromJson(<String, Object?>{'id': 'x', 'role': 'assistant', 'text': 'hi', 'time': 1000.0});
      expect(old.kind, ChatKind.text);
      expect(old.steps, isEmpty);
      final Session session = Session.fromJson(<String, Object?>{
        'id': 's', 'name': 'n', 'project': 'p', 'cwd': '/', 'entrypoint': 'cli', 'state': 'running', 'control': 'console', 'lastActivity': 1000.0,
        'activity': 'Editing main.swift',
        'facts': <String, Object?>{'contextTokens': 100000, 'contextWindow': 400000, 'limits': <Object?>[<String, Object?>{'label': '5h', 'percent': 62.0}]},
      });
      expect(session.activity, 'Editing main.swift');
      expect(session.facts!.contextFraction, 0.25);
      expect(session.facts!.limits.single.percent, 62);
    });
  });

  group('composer', () {
    Future<TextEditingController> pump(WidgetTester tester, String text) async {
      final TextEditingController controller = TextEditingController(text: text);
      addTearDown(controller.dispose);
      await tester.pumpWidget(host(Composer(
        controller: controller,
        send: const SendIdle(),
        viewOnly: false,
        onSend: () {},
        onRetry: () {},
        onDismissError: () {},
        commands: builtInSlashCommands,
        folder: '~/code/api',
      )));
      return controller;
    }

    testWidgets('a draft starting with ! shows shell mode: header, folder, hint', (WidgetTester tester) async {
      await pump(tester, '!ls -la');
      expect(find.text('Lệnh shell'), findsOneWidget);
      expect(find.textContaining('~/code/api'), findsOneWidget);
      expect(find.textContaining('xoá !'), findsOneWidget);
    });

    testWidgets('a wrapped command shows what will run', (WidgetTester tester) async {
      await pump(tester, '!echo one\n  two');
      expect(find.textContaining('Sẽ chạy', findRichText: true), findsOneWidget);
    });

    testWidgets('a plain draft has no shell header', (WidgetTester tester) async {
      await pump(tester, 'hello');
      expect(find.text('Lệnh shell'), findsNothing);
    });

    testWidgets('typing a slash lists commands; tapping one fills the field', (WidgetTester tester) async {
      final TextEditingController controller = await pump(tester, '/co');
      expect(find.text('/compact'), findsOneWidget);
      await tester.tap(find.text('/compact'));
      await tester.pumpAndSettle();
      expect(controller.text, '/compact ');
      expect(find.text('/compact'), findsNothing, reason: 'the list closes once there is a space');
    });
  });

  group('conversation', () {
    testWidgets('draws every kind of entry: shell, compaction, tool group, code, diff', (WidgetTester tester) async {
      await tester.pumpWidget(host(
        SizedBox(
          height: 5000,
          child: ConversationList(
            sessionId: 's2',
            entries: groupEntries(previewConversation()),
            pending: const <PendingSend>[],
            hasMore: false,
            loadingOlder: false,
            onLoadOlder: () {},
            onRetry: (String _) {},
            onDiscard: (String _) {},
            openGroups: const <String>{'tools:m4'},
          ),
        ),
      ));
      await tester.pump(const Duration(milliseconds: 400));
      expect(find.byType(ShellBubble), findsOneWidget);
      expect(find.byType(CompactionNotice), findsOneWidget);
      expect(find.byType(DiffView), findsOneWidget);
      expect(find.byType(CodeBlock), findsOneWidget);
      expect(tester.takeException(), isNull);
    });

    testWidgets('a new message or the working line keeps the rows already shown (same state, no rebuild from scratch)', (WidgetTester tester) async {
      ChatMessage message(String id, String text) =>
          ChatMessage(id: id, role: ChatRole.assistant, text: text, time: DateTime(2026, 11, 13));
      Widget list(List<ChatMessage> messages, {bool working = false}) => host(
            SizedBox(
              height: 1200,
              child: ConversationList(
                sessionId: 's1',
                entries: groupEntries(messages),
                pending: const <PendingSend>[],
                hasMore: false,
                loadingOlder: false,
                onLoadOlder: () {},
                onRetry: (String _) {},
                onDiscard: (String _) {},
                working: working,
              ),
            ),
          );
      final List<ChatMessage> first = <ChatMessage>[message('a', 'Câu **một**'), message('b', 'Câu **hai**')];
      await tester.pumpWidget(list(first));
      await tester.pump(const Duration(milliseconds: 400));
      State<StatefulWidget> stateOf(String text) =>
          tester.state(find.ancestor(of: find.textContaining(text, findRichText: true), matching: find.byType(MarkdownText)).first);
      final State<StatefulWidget> before = stateOf('một');

      await tester.pumpWidget(list(<ChatMessage>[...first, message('c', 'Câu ba')], working: true));
      await tester.pump(const Duration(milliseconds: 400));

      expect(identical(stateOf('một'), before), isTrue);
      expect(tester.takeException(), isNull);
    });

    testWidgets('a tool group is closed until tapped', (WidgetTester tester) async {
      final ToolGroupEntry group = groupEntries(previewConversation()).whereType<ToolGroupEntry>().first;
      await tester.pumpWidget(host(ToolGroupView(entry: group)));
      expect(find.text('Glob'), findsNothing);
      await tester.tap(find.text('4 bước công cụ'));
      await tester.pump();
      expect(find.text('Glob'), findsOneWidget);
    });
  });

  group('sessions list', () {
    Widget view(SessionsState state, {String query = '', String? project, Set<String> archived = const <String>{}, void Function(Session)? onTakeover}) => host(
          SizedBox(
            height: 2300,
            child: SessionsView(
              state: state,
              expanded: const <String>{'s2'},
              selectedId: null,
              archived: archived,
              query: query,
              project: project,
              onQuery: (String _) {},
              onProject: (String? _) {},
              onSelect: (Session _) {},
              onToggleExpand: (String _) {},
              onRefresh: () async {},
              onNewSession: () {},
              onArchive: (Session _) {},
              onTakeover: onTakeover ?? (Session _) {},
              now: previewNow,
            ),
          ),
        );

    testWidgets('groups in the kit order, with the live activity line and the subagent tree', (WidgetTester tester) async {
      await tester.pumpWidget(view(SessionsState(phase: LoadPhase.ready, sessions: previewSessions())));
      expect(find.text('CẦN BẠN'), findsOneWidget);
      expect(find.text('ĐANG CHẠY'), findsOneWidget);
      expect(find.textContaining('Editing PaymentService.swift', findRichText: true), findsOneWidget);
      expect(find.textContaining('Find every caller', findRichText: true), findsOneWidget);
      expect(find.textContaining('git push origin', findRichText: true), findsOneWidget, reason: 'what the needs-input session waits for');
    });

    testWidgets('filtering by project and archiving hide sessions', (WidgetTester tester) async {
      await tester.pumpWidget(view(SessionsState(phase: LoadPhase.ready, sessions: previewSessions()), project: 'api'));
      expect(find.text('Refactor payment module'), findsOneWidget);
      expect(find.text('Update README screenshots'), findsNothing);
      await tester.pumpWidget(view(SessionsState(phase: LoadPhase.ready, sessions: previewSessions()), archived: const <String>{'s2'}));
      expect(find.text('Refactor payment module'), findsNothing);
    });

    testWidgets('every state has its own screen: loading, error, empty, no match', (WidgetTester tester) async {
      await tester.pumpWidget(view(const SessionsState()));
      expect(find.byType(SkeletonRows), findsOneWidget);
      await tester.pumpWidget(view(const SessionsState(phase: LoadPhase.error)));
      expect(find.text('Thử lại'), findsOneWidget);
      await tester.pumpWidget(view(const SessionsState(phase: LoadPhase.ready)));
      expect(find.text('Chưa có session nào'), findsOneWidget);
      await tester.pumpWidget(view(SessionsState(phase: LoadPhase.ready, sessions: previewSessions()), query: 'zzzz'));
      expect(find.textContaining('zzzz'), findsOneWidget);
    });

    testWidgets('swiping right takes a view-only session over and keeps the row', (WidgetTester tester) async {
      final List<String> taken = <String>[];
      tester.view.physicalSize = const Size(800, 2400);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      await tester.pumpWidget(view(SessionsState(phase: LoadPhase.ready, sessions: previewSessions()), onTakeover: (Session s) => taken.add(s.id)));
      await tester.drag(find.text('nháp'), const Offset(400, 0));
      await tester.pumpAndSettle();
      expect(taken, <String>['s4']);
      expect(find.text('nháp'), findsOneWidget);
    });
  });
}

void meterTests() {
  group('meters', () {
    double fill(WidgetTester tester) => tester.widget<Transform>(find.byKey(const ValueKey<String>('meter-fill'))).transform.entry(0, 0);

    testWidgets('the fill is drawn (it has height) and follows the value, with a colour per level', (WidgetTester tester) async {
      Widget meter(double f) => host(KitMeter(label: 'Tuần', value: '${(f * 100).round()}%', fraction: f, width: 60));
      await tester.pumpWidget(meter(0.14));
      await tester.pumpAndSettle();
      expect(fill(tester), closeTo(0.14, 0.001));
      expect(tester.getSize(find.byKey(const ValueKey<String>('meter-fill'))).height, 3, reason: 'a child-less ColoredBox would be 0 tall');
      Color colour() => tester.widget<ColoredBox>(find.descendant(of: find.byKey(const ValueKey<String>('meter-fill')), matching: find.byType(ColoredBox))).color;
      expect(colour(), Kit.dark.working);
      await tester.pumpWidget(meter(0.62));
      await tester.pumpAndSettle();
      expect(fill(tester), closeTo(0.62, 0.001));
      await tester.pumpWidget(meter(0.85));
      await tester.pumpAndSettle();
      expect(colour(), Kit.dark.needs);
      await tester.pumpWidget(meter(0.97));
      await tester.pumpAndSettle();
      expect(colour(), Kit.dark.danger);
    });

    testWidgets('a newer session list changes the header fill, gliding there', (WidgetTester tester) async {
      Session withContext(int tokens) => previewSession(
            id: 's',
            name: 'Notching repo startup',
            project: 'p',
            state: SessionState.running,
            facts: SessionFacts(contextTokens: tokens, contextWindow: 400000, limits: const <LimitMeter>[LimitMeter(label: '5h', percent: 48)]),
          );
      Widget header(Session s) => host(ChatHeaderView(session: s, muted: false, onToggleMute: () {}));
      await tester.pumpWidget(header(withContext(100000)));
      await tester.pumpAndSettle();
      Finder contextFill() => find.byKey(const ValueKey<String>('meter-fill')).first;
      double value() => tester.widget<Transform>(contextFill()).transform.entry(0, 0);
      expect(value(), closeTo(0.25, 0.001));
      expect(find.textContaining('25%', findRichText: true), findsOneWidget);
      await tester.pumpWidget(header(withContext(246600)));
      await tester.pump(const Duration(milliseconds: 120));
      expect(value(), greaterThan(0.25));
      expect(value(), lessThan(0.6165), reason: 'still on its way');
      await tester.pumpAndSettle();
      expect(value(), closeTo(0.6165, 0.001));
      expect(find.textContaining('62%', findRichText: true), findsOneWidget);
    });
  });
}
