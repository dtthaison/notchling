import 'package:agent_console/app/shell.dart';
import 'package:agent_console/features/chat/chat_notifier.dart';
import 'package:agent_console/features/chat/composer.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/harness.dart';

void main() {
  Widget composer(TextEditingController controller, FocusNode focus, {double inset = 0, ValueNotifier<double>? insets}) {
    final KeyboardLiftController lift = KeyboardLiftController();
    addTearDown(lift.dispose);
    final Widget field = KeyboardLift(
      controller: lift,
      list: const SizedBox.expand(),
      composer: Composer(
        controller: controller,
        send: const SendIdle(),
        viewOnly: false,
        onSend: () {},
        onRetry: () {},
        onDismissError: () {},
        onMic: () {},
        focusNode: focus,
      ),
    );
    return host(insets == null
        ? SizedBox(height: 600, child: field)
        : ValueListenableBuilder<double>(
            valueListenable: insets,
            builder: (BuildContext context, double value, Widget? child) => MediaQuery(
              data: MediaQuery.of(context).copyWith(viewInsets: EdgeInsets.only(bottom: value)),
              child: child!,
            ),
            child: SizedBox(height: 600, child: field),
          ));
  }

  testWidgets('tapping the composer field focuses it', (WidgetTester tester) async {
    final TextEditingController controller = TextEditingController();
    final FocusNode focus = FocusNode();
    addTearDown(controller.dispose);
    addTearDown(focus.dispose);
    await tester.pumpWidget(composer(controller, focus));
    expect(focus.hasFocus, isFalse);
    await tester.tap(find.byType(TextField));
    await tester.pump();
    expect(focus.hasFocus, isTrue);
  });

  testWidgets('the keyboard coming up does not rebuild the composer and drop its focus', (WidgetTester tester) async {
    final TextEditingController controller = TextEditingController();
    final FocusNode focus = FocusNode();
    final ValueNotifier<double> insets = ValueNotifier<double>(0);
    addTearDown(controller.dispose);
    addTearDown(focus.dispose);
    addTearDown(insets.dispose);
    await tester.pumpWidget(composer(controller, focus, insets: insets));

    await tester.tap(find.byType(TextField));
    await tester.pump();
    expect(focus.hasFocus, isTrue);

    // The keyboard animates in over several frames.
    for (final double value in <double>[40, 160, 320, 0, 320]) {
      insets.value = value;
      await tester.pump(const Duration(milliseconds: 16));
      expect(focus.hasFocus, isTrue, reason: 'focus survives inset $value');
    }
    await tester.enterText(find.byType(TextField), 'chạy test');
    expect(controller.text, 'chạy test');
  });

  testWidgets('the field takes focus again after a dictation state change', (WidgetTester tester) async {
    final TextEditingController controller = TextEditingController();
    final FocusNode focus = FocusNode();
    addTearDown(controller.dispose);
    addTearDown(focus.dispose);
    await tester.pumpWidget(composer(controller, focus));
    await tester.tap(find.byType(TextField));
    await tester.pump();
    focus.unfocus();
    await tester.pump();
    expect(focus.hasFocus, isFalse);
    await tester.tap(find.byType(TextField));
    await tester.pump();
    expect(focus.hasFocus, isTrue);
  });
}
