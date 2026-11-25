import 'package:agent_console/app/shell.dart' show KeyboardLift, KeyboardLiftController;
import 'package:agent_console/app/widgets/kit.dart';
import 'package:agent_console/features/chat/chat_notifier.dart';
import 'package:agent_console/features/chat/composer.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/harness.dart';

void main() {
  const double screen = 800;
  const double bar = 24;
  late ValueNotifier<double> insets;
  late ValueNotifier<double> padding;
  late KeyboardLiftController lift;

  Future<void> pump(WidgetTester tester, {required bool decoupled}) async {
    tester.view.physicalSize = const Size(392 * 2.75, screen * 2.75);
    tester.view.devicePixelRatio = 2.75;
    addTearDown(tester.view.reset);
    insets = ValueNotifier<double>(0);
    addTearDown(insets.dispose);
    padding = ValueNotifier<double>(bar);
    addTearDown(padding.dispose);
    lift = KeyboardLiftController();
    addTearDown(lift.dispose);
    final TextEditingController text = TextEditingController();
    final FocusNode focus = FocusNode();
    addTearDown(text.dispose);
    addTearDown(focus.dispose);
    await tester.pumpWidget(host(
      Scaffold(
        resizeToAvoidBottomInset: false,
        body: ListenableBuilder(
          listenable: Listenable.merge(<Listenable>[insets, padding]),
          builder: (BuildContext context, Widget? child) => MediaQuery(
            data: MediaQuery.of(context).copyWith(
              padding: EdgeInsets.only(bottom: padding.value),
              viewPadding: EdgeInsets.only(bottom: padding.value),
              viewInsets: EdgeInsets.only(bottom: insets.value),
            ),
            child: child!,
          ),
          child: SizedBox(
            height: screen,
            child: KeyboardLift(
              controller: lift,
              decoupled: decoupled,
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
        ),
      ),
      screen: true,
    ));
    await tester.pump();
  }

  double cardBottom(WidgetTester tester) => tester.getBottomLeft(find.byWidgetPredicate((Widget w) => w is Surface && w.level == 2).first).dy;

  testWidgets('on Android the composer is never ahead of a rising keyboard, then slides onto it', (WidgetTester tester) async {
    await pump(tester, decoupled: true);
    final double rest = cardBottom(tester);
    for (double inset = 30; inset <= 300; inset += 30) {
      insets.value = inset;
      await tester.pump(const Duration(milliseconds: 8));
      expect(cardBottom(tester), closeTo(rest, 0.5), reason: 'inset $inset: waits while the keyboard moves');
    }
    await tester.pump(const Duration(milliseconds: 60));
    await tester.pump(const Duration(milliseconds: 100));
    final double middle = cardBottom(tester);
    expect(middle, lessThan(rest - 1), reason: 'the slide has started');
    expect(middle, greaterThan((screen - 300) + 8.5), reason: 'and is not there yet: a slide, not a jump');
    await tester.pump(const Duration(milliseconds: 400));
    final double gap = (screen - 300) - cardBottom(tester);
    expect(gap, inInclusiveRange(0, 8.5), reason: 'the card ends on the keyboard');
    expect(lift.settled.value, greaterThan(0), reason: 'the list makes room once the slide has ended');
  });

  testWidgets('a keyboard that jumps straight to its full height still gets the slide', (WidgetTester tester) async {
    await pump(tester, decoupled: true);
    final double rest = cardBottom(tester);
    insets.value = 300;
    await tester.pump(const Duration(milliseconds: 8));
    expect(cardBottom(tester), closeTo(rest, 0.5), reason: 'not ahead of the keyboard on the first frame');
    await tester.pump(const Duration(milliseconds: 60));
    await tester.pump(const Duration(milliseconds: 100));
    final double middle = cardBottom(tester);
    expect(middle, lessThan(rest - 1));
    expect(middle, greaterThan((screen - 300) + 8.5));
    await tester.pump(const Duration(milliseconds: 400));
    expect((screen - 300) - cardBottom(tester), inInclusiveRange(0, 8.5));
  });

  testWidgets('a keyboard that opens to the height it had before starts the slide at once', (WidgetTester tester) async {
    await pump(tester, decoupled: true);
    insets.value = 300;
    await tester.pump(const Duration(milliseconds: 8));
    await tester.pump(const Duration(milliseconds: 60));
    await tester.pump(const Duration(milliseconds: 16));
    await tester.pump(const Duration(milliseconds: 400));
    insets.value = 0;
    await tester.pump(const Duration(milliseconds: 8));
    await tester.pump(const Duration(milliseconds: 400));
    final double rest = cardBottom(tester);
    insets.value = 300;
    await tester.pump(const Duration(milliseconds: 8));
    await tester.pump(const Duration(milliseconds: 16));
    expect(cardBottom(tester), lessThan(rest - 0.5), reason: 'no wait: the height is known');
  });

  testWidgets('closing slides the page down at once and ends at rest', (WidgetTester tester) async {
    await pump(tester, decoupled: true);
    final double rest = cardBottom(tester);
    insets.value = 300;
    await tester.pump(const Duration(milliseconds: 8));
    await tester.pump(const Duration(milliseconds: 60));
    await tester.pump(const Duration(milliseconds: 16));
    await tester.pump(const Duration(milliseconds: 400));
    final double open = cardBottom(tester);
    insets.value = 0;
    await tester.pump(const Duration(milliseconds: 8));
    await tester.pump(const Duration(milliseconds: 60));
    final double middle = cardBottom(tester);
    expect(middle, greaterThan(open + 0.5));
    expect(middle, lessThan(rest - 0.5));
    await tester.pump(const Duration(milliseconds: 400));
    expect(cardBottom(tester), closeTo(rest, 0.5));
    expect(lift.settled.value, 0);
  });

  testWidgets('off Android the composer still follows the inset every frame', (WidgetTester tester) async {
    await pump(tester, decoupled: false);
    insets.value = 200;
    await tester.pump(const Duration(milliseconds: 8));
    expect((screen - 200) - cardBottom(tester), inInclusiveRange(0, 8.5));
  });

  Future<void> openKeyboard(WidgetTester tester) async {
    insets.value = 300;
    await tester.pump(const Duration(milliseconds: 8));
    await tester.pump(const Duration(milliseconds: 60));
    await tester.pump(const Duration(milliseconds: 16));
    await tester.pump(const Duration(milliseconds: 400));
  }

  testWidgets('a dip in the inset that comes back within the still time leaves the list made room for', (WidgetTester tester) async {
    await pump(tester, decoupled: true);
    await openKeyboard(tester);
    final double open = cardBottom(tester);
    final double made = lift.settled.value;
    expect(made, greaterThan(0));
    insets.value = 260;
    await tester.pump(const Duration(milliseconds: 8));
    insets.value = 300;
    await tester.pump(const Duration(milliseconds: 8));
    await tester.pump(const Duration(milliseconds: 400));
    expect(cardBottom(tester), closeTo(open, 0.5));
    expect(lift.settled.value, made, reason: 'room is made again once the card is back on the keyboard');
  });

  testWidgets('a spike in the inset that comes back within the still time leaves the list made room for', (WidgetTester tester) async {
    await pump(tester, decoupled: true);
    await openKeyboard(tester);
    final double open = cardBottom(tester);
    final double made = lift.settled.value;
    expect(made, greaterThan(0));
    insets.value = 305;
    await tester.pump(const Duration(milliseconds: 8));
    insets.value = 300;
    await tester.pump(const Duration(milliseconds: 8));
    await tester.pump(const Duration(milliseconds: 400));
    expect(cardBottom(tester), closeTo(open, 0.5));
    expect(lift.settled.value, made, reason: 'room stays made whichever way the inset moved first');
  });

  testWidgets('a spike and a dip in a row, back within the still time, leave the list made room for', (WidgetTester tester) async {
    await pump(tester, decoupled: true);
    await openKeyboard(tester);
    final double open = cardBottom(tester);
    final double made = lift.settled.value;
    insets.value = 305;
    await tester.pump(const Duration(milliseconds: 8));
    insets.value = 260;
    await tester.pump(const Duration(milliseconds: 8));
    insets.value = 300;
    await tester.pump(const Duration(milliseconds: 8));
    await tester.pump(const Duration(milliseconds: 400));
    expect(cardBottom(tester), closeTo(open, 0.5));
    expect(lift.settled.value, made);
  });

  testWidgets('a dip in the inset that outlasts the still time slides to the new height and back', (WidgetTester tester) async {
    await pump(tester, decoupled: true);
    await openKeyboard(tester);
    final double open = cardBottom(tester);
    final double made = lift.settled.value;
    insets.value = 260;
    await tester.pump(const Duration(milliseconds: 8));
    await tester.pump(const Duration(milliseconds: 60));
    await tester.pump(const Duration(milliseconds: 400));
    expect(cardBottom(tester), closeTo(open + 40, 0.5));
    insets.value = 300;
    await tester.pump(const Duration(milliseconds: 8));
    await tester.pump(const Duration(milliseconds: 60));
    await tester.pump(const Duration(milliseconds: 400));
    expect(cardBottom(tester), closeTo(open, 0.5));
    expect(lift.settled.value, made);
  });

  testWidgets('on Android a change of the bottom padding under a steady keyboard moves the card', (WidgetTester tester) async {
    await pump(tester, decoupled: true);
    await openKeyboard(tester);
    final double open = cardBottom(tester);
    padding.value = bar + 16;
    await tester.pump(const Duration(milliseconds: 8));
    await tester.pump(const Duration(milliseconds: 60));
    await tester.pump(const Duration(milliseconds: 400));
    expect(cardBottom(tester), closeTo(open, 0.5), reason: 'the card stays on the keyboard');
    expect(lift.settled.value, greaterThan(0), reason: 'the list is made room for again at the new height');
    padding.value = bar;
    await tester.pump(const Duration(milliseconds: 8));
    await tester.pump(const Duration(milliseconds: 60));
    await tester.pump(const Duration(milliseconds: 400));
    expect(cardBottom(tester), closeTo(open, 0.5));
    expect(lift.settled.value, greaterThan(0));
  });

  testWidgets('off Android a change of the bottom padding under a steady keyboard moves the card at once', (WidgetTester tester) async {
    await pump(tester, decoupled: false);
    insets.value = 300;
    await tester.pump(const Duration(milliseconds: 8));
    await tester.pump(const Duration(milliseconds: 400));
    final double open = cardBottom(tester);
    padding.value = bar + 16;
    await tester.pump(const Duration(milliseconds: 8));
    expect(cardBottom(tester), closeTo(open, 0.5));
  });
}
