import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/chat/composer_controls.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/harness.dart';

class _Probe extends StatefulWidget {
  const _Probe({
    super.key,
    required this.sent,
    this.model = 'claude-opus-5-5',
    this.effort = 'medium',
    this.models,
  });
  final List<String> sent;
  final String? model;
  final String? effort;
  final List<ModelChoice>? models;

  @override
  State<_Probe> createState() => _ProbeState();
}

class _ProbeState extends State<_Probe> {
  late String? model = widget.model;
  late String? effort = widget.effort;
  late List<ModelChoice>? models = widget.models;

  void update({String? model, String? effort}) => setState(() {
    this.model = model ?? this.model;
    this.effort = effort ?? this.effort;
  });

  @override
  Widget build(BuildContext context) => ComposerControls(
    model: model,
    effort: effort,
    models: models,
    enabled: true,
    onSend: widget.sent.add,
  );
}

void main() {
  Finder chip(String key) => find.byKey(ValueKey<String>(key));

  Future<GlobalKey<_ProbeState>> pumpProbe(
    WidgetTester tester,
    List<String> sent, {
    String? model = 'claude-opus-5-5',
    String? effort = 'medium',
    List<ModelChoice>? models,
  }) async {
    final GlobalKey<_ProbeState> key = GlobalKey<_ProbeState>();
    await tester.pumpWidget(
      host(
        _Probe(
          key: key,
          sent: sent,
          model: model,
          effort: effort,
          models: models,
        ),
      ),
    );
    return key;
  }

  Future<void> pickEffort(WidgetTester tester, String level) async {
    await tester.tap(chip('effort-chip'));
    await tester.pumpAndSettle();
    await tester.tap(chip('choice-$level'));
    await tester.pumpAndSettle();
  }

  testWidgets('a pick shows as applying and falls back to the confirmed effort after the timeout', (
    WidgetTester tester,
  ) async {
    final List<String> sent = <String>[];
    await pumpProbe(tester, sent);
    expect(find.text('Medium'), findsOneWidget);
    expect(find.text('Applying'), findsNothing);

    await pickEffort(tester, 'high');
    expect(sent, <String>['/effort high']);
    expect(find.text('High'), findsOneWidget);
    expect(find.text('Applying'), findsOneWidget);

    await tester.pump(pendingPickTimeout + const Duration(seconds: 1));
    expect(find.text('Medium'), findsOneWidget);
    expect(find.text('High'), findsNothing);
    expect(find.text('Applying'), findsNothing);
  });

  testWidgets('the session reporting the picked effort ends the wait at once', (
    WidgetTester tester,
  ) async {
    final GlobalKey<_ProbeState> key = await pumpProbe(tester, <String>[]);
    await pickEffort(tester, 'high');
    expect(find.text('Applying'), findsOneWidget);

    key.currentState!.update(effort: 'high');
    await tester.pump();
    expect(find.text('High'), findsOneWidget);
    expect(find.text('Applying'), findsNothing);
    await tester.pump(pendingPickTimeout + const Duration(seconds: 1));
    expect(find.text('High'), findsOneWidget);
  });

  testWidgets('a session that reports another effort wins over the pick', (
    WidgetTester tester,
  ) async {
    final GlobalKey<_ProbeState> key = await pumpProbe(tester, <String>[]);
    await pickEffort(tester, 'high');
    key.currentState!.update(effort: 'low');
    await tester.pump();
    expect(find.text('Low'), findsOneWidget);
    expect(find.text('High'), findsNothing);
    expect(find.text('Applying'), findsNothing);
  });

  testWidgets('a Mac that sends no effort never leaves the pick looking set', (
    WidgetTester tester,
  ) async {
    await pumpProbe(tester, <String>[], effort: null);
    expect(find.text('Effort'), findsOneWidget);

    await pickEffort(tester, 'xhigh');
    expect(find.text('Extra High'), findsOneWidget);
    expect(find.text('Applying'), findsOneWidget);

    await tester.pump(pendingPickTimeout + const Duration(seconds: 1));
    expect(find.text('Extra High'), findsNothing);
    expect(find.text('Effort'), findsOneWidget);
    expect(find.text('Applying'), findsNothing);
  });

  testWidgets('a model pick is pending too and falls back', (
    WidgetTester tester,
  ) async {
    final List<String> sent = <String>[];
    await pumpProbe(tester, sent);
    await tester.tap(chip('model-chip'));
    await tester.pumpAndSettle();
    await tester.tap(chip('choice-claude-sonnet-5-5'));
    await tester.pumpAndSettle();
    expect(sent, <String>['/model claude-sonnet-5-5']);
    expect(find.text('Sonnet 5.5'), findsOneWidget);
    expect(find.text('Applying'), findsOneWidget);

    await tester.pump(pendingPickTimeout + const Duration(seconds: 1));
    expect(find.text('Opus 5.5'), findsOneWidget);
    expect(find.text('Sonnet 5.5'), findsNothing);
  });

  testWidgets("the model sheet lists the Mac's catalog when it sends one", (
    WidgetTester tester,
  ) async {
    final List<String> sent = <String>[];
    await pumpProbe(
      tester,
      sent,
      model: 'claude-fable-9',
      models: const <ModelChoice>[
        ModelChoice(id: 'claude-fable-9', name: 'Fable Nine'),
        ModelChoice(id: 'claude-opus-9', name: 'Opus Nine'),
      ],
    );
    expect(find.text('Fable Nine'), findsOneWidget);

    await tester.tap(chip('model-chip'));
    await tester.pumpAndSettle();
    expect(find.text('Opus Nine'), findsOneWidget);
    expect(chip('choice-claude-opus-9'), findsOneWidget);
    expect(chip('choice-claude-opus-5-5'), findsNothing);
    expect(chip('choice-claude-haiku-4-5'), findsNothing);
    expect(find.text('More models'), findsNothing);

    await tester.tap(chip('choice-claude-opus-9'));
    await tester.pumpAndSettle();
    expect(sent, <String>['/model claude-opus-9']);
  });

  testWidgets('without a catalog, or with an empty one, the built-in list is the fallback', (
    WidgetTester tester,
  ) async {
    for (final List<ModelChoice>? models in <List<ModelChoice>?>[
      null,
      const <ModelChoice>[],
    ]) {
      await pumpProbe(tester, <String>[], models: models);
      await tester.tap(chip('model-chip'));
      await tester.pumpAndSettle();
      expect(chip('choice-claude-opus-5-5'), findsOneWidget);
      expect(find.text('More models'), findsOneWidget);
      await tester.tapAt(const Offset(10, 10));
      await tester.pumpAndSettle();
    }
  });

  test('a models list from the wire keeps ids and names and ignores junk', () {
    expect(ModelChoice.listOrNull(null), isNull);
    expect(ModelChoice.listOrNull(<Object?>[]), isNull);
    final List<ModelChoice> models = ModelChoice.listOrNull(<Object?>[
      <String, Object?>{'id': 'claude-opus-5-5', 'name': 'Opus 5.5'},
      <String, Object?>{'id': 'claude-x'},
      <String, Object?>{'name': 'no id'},
      'junk',
    ])!;
    expect(models.map((ModelChoice m) => m.id), <String>['claude-opus-5-5', 'claude-x']);
    expect(models.map((ModelChoice m) => m.name), <String>['Opus 5.5', 'claude-x']);
  });
}
