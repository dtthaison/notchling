import 'dart:ui' as ui;

import 'package:flutter/widgets.dart';

const bool kbTraceOn = bool.fromEnvironment('KB_TRACE');

/// Profiling aid, off unless built with `--dart-define=KB_TRACE=true`: logs (tag KBTRACE, epoch ms on the phone's
/// clock) when a pointer goes down on the composer, and each change of the keyboard inset, so the delay between a
/// tap and the keyboard can be read against the IME's own log lines.
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
    // The first few changes of each open or close are enough to read the start of the animation.
    if (_last == 0 || inset == 0) _logged = 0;
    _last = inset;
    if (_logged++ < 4) debugPrint('KBTRACE inset=${inset.round()} t=${DateTime.now().millisecondsSinceEpoch}');
  }
}

/// Wraps the composer so a pointer-down is logged when tracing is on; transparent otherwise.
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
