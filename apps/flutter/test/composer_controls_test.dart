import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/chat/chat_notifier.dart';
import 'package:agent_console/features/chat/composer.dart';
import 'package:agent_console/features/chat/composer_controls.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/harness.dart';

void main() {
  Future<List<String>> pumpControls(
    WidgetTester tester, {
    bool enabled = true,
    double scale = 1,
  }) async {
    final TextEditingController controller = TextEditingController();
    addTearDown(controller.dispose);
    final List<String> sent = <String>[];
    await tester.pumpWidget(
      host(
        Composer(
          controller: controller,
          send: const SendIdle(),
          viewOnly: false,
          onSend: () {},
          onRetry: () {},
          onDismissError: () {},
          model: 'claude-opus-5-5',
          effort: 'medium',
          onSendText: enabled ? sent.add : null,
          onAttach: () {},
        ),
        textScale: scale,
      ),
    );
    return sent;
  }

  test('a model id reads as its name', () {
    expect(modelShortName('claude-opus-5-5'), 'Opus 5.5');
    expect(modelShortName('claude-fable-5-2'), 'Fable 5.2');
    expect(modelShortName('claude-sonnet-5'), 'Sonnet 5');
    expect(modelShortName('claude-haiku-4-5'), 'Haiku 4.5');
  });

  test('a session with no tool is a Claude session and facts carry the effort', () {
    final Session session = Session.fromJson(<String, Object?>{
      'id': 's',
      'name': 'n',
      'project': 'p',
      'cwd': '/',
      'entrypoint': 'cli',
      'state': 'running',
      'control': 'console',
      'lastActivity': 1000.0,
      'facts': <String, Object?>{'limits': <Object?>[], 'effort': 'xhigh'},
    });
    expect(session.tool, 'claude');
    expect(session.facts!.effort, 'xhigh');
    final Session older = Session.fromJson(<String, Object?>{
      'id': 's',
      'name': 'n',
      'project': 'p',
      'cwd': '/',
      'entrypoint': 'cli',
      'state': 'running',
      'control': 'console',
      'lastActivity': 1000.0,
      'tool': 'codex',
      'facts': <String, Object?>{'limits': <Object?>[]},
    });
    expect(older.tool, 'codex');
    expect(older.facts!.effort, isNull);
  });

  testWidgets('picking a model sends /model with the full id and the chip shows its name', (WidgetTester tester) async {
    final List<String> sent = await pumpControls(tester);
    expect(find.text('Opus 5.5'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey<String>('model-chip')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey<String>('choice-claude-sonnet-5-5')));
    await tester.pumpAndSettle();
    expect(sent, <String>['/model claude-sonnet-5-5']);
    expect(find.text('Sonnet 5.5'), findsOneWidget);
  });

  testWidgets('picking an effort sends /effort with the level', (WidgetTester tester) async {
    final List<String> sent = await pumpControls(tester);
    await tester.tap(find.byKey(const ValueKey<String>('effort-chip')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey<String>('choice-high')));
    await tester.pumpAndSettle();
    expect(sent, <String>['/effort high']);
  });

  testWidgets('compact asks first and sends /compact only after the confirm', (WidgetTester tester) async {
    final List<String> sent = await pumpControls(tester);
    await tester.tap(find.byKey(const ValueKey<String>('compact-button')));
    await tester.pumpAndSettle();
    expect(sent, isEmpty);
    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();
    expect(sent, isEmpty);
    await tester.tap(find.byKey(const ValueKey<String>('compact-button')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Compact').last);
    await tester.pumpAndSettle();
    expect(sent, <String>['/compact']);
  });

  testWidgets('the controls are hidden when the session cannot take them', (WidgetTester tester) async {
    await pumpControls(tester, enabled: false);
    expect(find.byKey(const ValueKey<String>('model-chip')), findsNothing);
    expect(find.byKey(const ValueKey<String>('compact-button')), findsNothing);
  });

  testWidgets('a large text size does not overflow the toolbar', (WidgetTester tester) async {
    await pumpControls(tester, scale: 1.3);
    expect(tester.takeException(), isNull);
  });
}
