import 'package:agent_console/app/widgets/motion.dart';
import 'package:agent_console/features/chat/chat_parts.dart';
import 'package:agent_console/features/chat/markdown_text.dart';
import 'package:agent_console/features/sessions/status_style.dart';
import 'package:agent_console/core/protocol/models.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/harness.dart';
import 'support/preview_data.dart';

Widget _withMotion(Widget child, {required bool reduced}) =>
    host(MediaQuery(data: MediaQueryData(disableAnimations: reduced), child: child));

void main() {
  group('ExpandReveal', () {
    testWidgets('grows over time with motion, and at once with Reduce Motion', (WidgetTester tester) async {
      Widget build(bool open) => Align(alignment: Alignment.topLeft, child: ExpandReveal(expanded: open, child: const SizedBox(width: 100, height: 100)));
      await tester.pumpWidget(host(build(false)));
      await tester.pumpWidget(host(build(true)));
      await tester.pump(const Duration(milliseconds: 60));
      final double partway = tester.getSize(find.byType(ExpandReveal)).height;
      expect(partway, greaterThan(0));
      expect(partway, lessThan(100), reason: 'still moving after 60 ms');
      await tester.pumpAndSettle();
      expect(tester.getSize(find.byType(ExpandReveal)).height, 100);
    });

    testWidgets('Reduce Motion: the final size is there on the next frame', (WidgetTester tester) async {
      Widget build(bool open) => _withMotion(Align(alignment: Alignment.topLeft, child: ExpandReveal(expanded: open, child: const SizedBox(width: 100, height: 100))), reduced: true);
      await tester.pumpWidget(build(false));
      await tester.pumpWidget(build(true));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 1));
      expect(tester.getSize(find.byType(ExpandReveal)).height, 100);
    });
  });

  group('Entrance', () {
    testWidgets('fades in and rises, and with Reduce Motion does not move', (WidgetTester tester) async {
      await tester.pumpWidget(host(const Entrance(child: SizedBox(key: ValueKey<String>('x'), width: 50, height: 50))));
      final double start = tester.getTopLeft(find.byKey(const ValueKey<String>('x'))).dy;
      await tester.pump(const Duration(milliseconds: 16));
      final double early = tester.getTopLeft(find.byKey(const ValueKey<String>('x'))).dy;
      await tester.pumpAndSettle();
      final double end = tester.getTopLeft(find.byKey(const ValueKey<String>('x'))).dy;
      expect(early, greaterThan(end), reason: 'it starts lower and rises');
      expect(start, greaterThanOrEqualTo(early));
      await tester.pumpWidget(_withMotion(const Entrance(child: SizedBox(key: ValueKey<String>('y'), width: 50, height: 50)), reduced: true));
      await tester.pump(const Duration(milliseconds: 16));
      final double still = tester.getTopLeft(find.byKey(const ValueKey<String>('y'))).dy;
      await tester.pumpAndSettle();
      expect(tester.getTopLeft(find.byKey(const ValueKey<String>('y'))).dy, still);
    });
  });

  group('status', () {
    testWidgets('the pill crossfades to the new state: both words are there mid-way, only the new one at the end', (WidgetTester tester) async {
      await tester.pumpWidget(host(const StatePill(state: SessionState.running)));
      await tester.pumpWidget(host(const StatePill(state: SessionState.needsInput)));
      await tester.pump(const Duration(milliseconds: 80));
      expect(find.text('Đang chạy'), findsOneWidget);
      expect(find.text('Cần bạn'), findsOneWidget);
      await tester.pumpAndSettle();
      expect(find.text('Đang chạy'), findsNothing);
      expect(find.text('Cần bạn'), findsOneWidget);
    });
  });

  group('FlipOnMove', () {
    testWidgets('a row that is laid out lower after an update starts at its old place and glides', (WidgetTester tester) async {
      Widget build(double gap) => host(
            SingleChildScrollView(
              child: Column(children: <Widget>[
                SizedBox(height: gap),
                FlipOnMove(child: SizedBox(key: const ValueKey<String>('row'), height: 40, width: 100)),
              ]),
            ),
          );
      await tester.pumpWidget(build(10));
      await tester.pump();
      await tester.pumpWidget(build(110));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 40));
      final double mid = tester.getTopLeft(find.byKey(const ValueKey<String>('row'))).dy;
      await tester.pumpAndSettle();
      final double end = tester.getTopLeft(find.byKey(const ValueKey<String>('row'))).dy;
      expect(mid, lessThan(end), reason: 'on its way down from the old place');
    });
  });

  group('copy button', () {
    testWidgets('shows a check and Đã chép after a copy, then goes back', (WidgetTester tester) async {
      await tester.pumpWidget(host(const CodeBlock(code: 'print(1);', language: 'dart')));
      expect(find.text('Chép'), findsOneWidget);
      await tester.tap(find.text('Chép'));
      await tester.pump(const Duration(milliseconds: 200));
      expect(find.text('Đã chép'), findsOneWidget);
      expect(find.byIcon(Icons.check), findsOneWidget);
      await tester.pump(const Duration(milliseconds: 2000));
      await tester.pumpAndSettle();
      expect(find.text('Chép'), findsOneWidget);
    });
  });

  group('tool group', () {
    testWidgets('opens smoothly: steps are absent, then present, with the chevron turned', (WidgetTester tester) async {
      final ToolGroupEntry group = groupEntries(previewConversation()).whereType<ToolGroupEntry>().first;
      await tester.pumpWidget(host(ToolGroupView(entry: group)));
      expect(find.text('Glob'), findsNothing);
      await tester.tap(find.text('4 bước công cụ'));
      await tester.pump(const Duration(milliseconds: 120));
      expect(find.text('Glob'), findsOneWidget);
      await tester.pumpAndSettle();
    });
  });
}
