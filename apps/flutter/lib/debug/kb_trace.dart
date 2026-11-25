import 'dart:ui' as ui;

import 'package:flutter/widgets.dart';

const bool kbTraceOn = bool.fromEnvironment('KB_TRACE');

void installKbTrace() {
  if (!kbTraceOn) return;
  WidgetsBinding.instance.addObserver(_KbObserver());
}

class _KbObserver with WidgetsBindingObserver {
  double _last = 0;
  int _logged = 0;

  @override
  void didChangeMetrics() {
    final double inset = WidgetsBinding.instance.platformDispatcher.views.fold<double>(0, (double m, ui.FlutterView v) => v.viewInsets.bottom > m ? v.viewInsets.bottom : m);
    if (inset == _last) return;
    if (_last == 0 || inset == 0) _logged = 0;
    _last = inset;
    if (_logged++ < 4) debugPrint('KBTRACE inset=${inset.round()} t=${DateTime.now().millisecondsSinceEpoch}');
  }
}

class KbTracePointer extends StatelessWidget {
  const KbTracePointer({super.key, required this.child});
  final Widget child;

  @override
  Widget build(BuildContext context) {
    if (!kbTraceOn) return child;
    return Listener(
      behavior: HitTestBehavior.translucent,
      onPointerDown: (PointerDownEvent e) => debugPrint('KBTRACE pointerdown t=${DateTime.now().millisecondsSinceEpoch}'),
      onPointerUp: (PointerUpEvent e) => debugPrint('KBTRACE pointerup t=${DateTime.now().millisecondsSinceEpoch}'),
      child: child,
    );
  }
}
