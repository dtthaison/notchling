import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:agent_console/app/shell.dart' show KeyboardLift, KeyboardLiftController;

import 'support/harness.dart';

void main() {
  testWidgets('closing the keyboard never leaves an empty band at the top of the list', (WidgetTester tester) async {
    final KeyboardLiftController lift = KeyboardLiftController();
    addTearDown(lift.dispose);
    final ValueNotifier<double> insets = ValueNotifier<double>(0);
    addTearDown(insets.dispose);
    final Widget page = ValueListenableBuilder<double>(
      valueListenable: insets,
      builder: (BuildContext context, double value, Widget? child) => MediaQuery(
        data: MediaQuery.of(context).copyWith(viewInsets: EdgeInsets.only(bottom: value)),
        child: child!,
      ),
      child: SizedBox(
        height: 600,
        child: KeyboardLift(
          controller: lift,
          list: ValueListenableBuilder<double>(
            valueListenable: lift.settled,
            builder: (BuildContext context, double room, Widget? _) => Padding(
              padding: EdgeInsets.only(bottom: room),
              child: const SizedBox.expand(key: ValueKey<String>('list')),
            ),
          ),
          composer: const SizedBox(height: 60),
        ),
      ),
    );
    await tester.pumpWidget(host(page));
    // Open, then let it settle: the room is made in the real layout.
    for (double v = 0; v <= 300; v += 30) {
      insets.value = v;
      await tester.pump(const Duration(milliseconds: 8));
    }
    await tester.pump(const Duration(milliseconds: 200));
    expect(lift.settled.value, 300);

    // Close: from the second frame on, the list's content bottom (room, then transform) must follow the keyboard
    // and no top band may open, i.e. the translation of the list is never positive.
    double worstBand = 0;
    for (double v = 270; v >= 0; v -= 30) {
      insets.value = v;
      await tester.pump(const Duration(milliseconds: 8));
      final double band = tester.widget<Transform>(find.ancestor(of: find.byKey(const ValueKey<String>('list')), matching: find.byType(Transform)).first).transform.getTranslation().y;
      if (v < 240 && band > worstBand) worstBand = band;
    }
    await tester.pump(const Duration(milliseconds: 200));
    expect(worstBand, lessThanOrEqualTo(0.5));
    expect(lift.settled.value, 0);
    unawaited(Future<void>.value());
  });
}
