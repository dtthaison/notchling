import 'package:agent_console/app/shell.dart' show KeyboardLift, KeyboardLiftController;
import 'package:agent_console/app/widgets/kit.dart';
import 'package:agent_console/features/chat/chat_notifier.dart';
import 'package:agent_console/features/chat/composer.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/harness.dart';

void main() {
  /// A full-screen Chat the way the shell builds it: the page extends to the screen bottom, the tab bar and gesture bar
  /// are in the MediaQuery's bottom padding, the composer pads itself with it.
  Future<void> pump(WidgetTester tester, ValueNotifier<double> insets, {required double screenHeight, required double bottomPadding}) async {
    final KeyboardLiftController lift = KeyboardLiftController();
    addTearDown(lift.dispose);
    final TextEditingController text = TextEditingController();
    final FocusNode focus = FocusNode();
    addTearDown(text.dispose);
    addTearDown(focus.dispose);
    await tester.pumpWidget(host(
      Scaffold(resizeToAvoidBottomInset: false, body: ValueListenableBuilder<double>(
        valueListenable: insets,
        builder: (BuildContext context, double inset, Widget? child) => MediaQuery(
          data: MediaQuery.of(context).copyWith(
            padding: EdgeInsets.only(bottom: bottomPadding),
            viewPadding: EdgeInsets.only(bottom: bottomPadding),
            viewInsets: EdgeInsets.only(bottom: inset),
          ),
          child: child!,
        ),
        child: SizedBox(
          height: screenHeight,
          child: KeyboardLift(
            controller: lift,
            list: const SizedBox.expand(),
            composer: Composer(
              controller: text,
              send: const SendIdle(),
              viewOnly: false,
              onSend: () {},
              onRetry: () {},
              onDismissError: () {},
              onMic: () {},
              focusNode: focus,
            ),
          ),
        ),
      )),
      screen: true,
    ));
    await tester.pump();
  }

  double cardBottom(WidgetTester tester) => tester.getBottomLeft(find.byWidgetPredicate((Widget w) => w is Surface && w.level == 2).first).dy;

  for (final (String, double) bar in <(String, double)>[('gesture bar and tab bar', 78), ('no bars', 0), ('gesture bar only', 24)]) {
    testWidgets('the composer card sits on the keyboard, opening and open (${bar.$1})', (WidgetTester tester) async {
      tester.view.physicalSize = const Size(392 * 2.75, 800 * 2.75);
      tester.view.devicePixelRatio = 2.75;
      addTearDown(tester.view.reset);
      final ValueNotifier<double> insets = ValueNotifier<double>(0);
      addTearDown(insets.dispose);
      await pump(tester, insets, screenHeight: 800, bottomPadding: bar.$2);
      final double rest = cardBottom(tester);
      expect(rest, closeTo(800 - bar.$2 - 8, 1.5), reason: 'at rest it clears the bars');
      for (final double inset in <double>[40, 120, 200, 300]) {
        insets.value = inset;
        await tester.pump(const Duration(milliseconds: 8));
        final double gap = (800 - inset) - cardBottom(tester);
        if (inset > bar.$2) {
          expect(gap, inInclusiveRange(0, 8.5), reason: 'inset $inset: composer card must be at most 8 dp above the keyboard, was $gap');
        } else {
          expect(gap, greaterThanOrEqualTo(0), reason: 'inset $inset: never under the keyboard');
        }
      }
      await tester.pump(const Duration(milliseconds: 300));
      expect((800 - 300) - cardBottom(tester), inInclusiveRange(0, 8.5), reason: 'fully open');
    });
  }
}
