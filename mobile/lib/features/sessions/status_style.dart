import 'dart:async';
import 'dart:ui' as ui;

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';

import '../../app/theme/tokens.dart';
import '../../app/widgets/common.dart';
import '../../core/protocol/models.dart';
import '../../l10n/app_localizations.dart';

extension SessionStateStyle on SessionState {
  Color get color => switch (this) {
        SessionState.needsInput => Token.statusInput,
        SessionState.running => Token.statusRunning,
        SessionState.done => Token.muted,
        SessionState.exited => Token.statusExited,
      };

  /// Text never uses `status-exited` (3.7:1): the dot does, the words use muted.
  Color get labelColor => this == SessionState.exited ? Token.muted : color;

  IconData get icon => switch (this) {
        SessionState.needsInput => Icons.mark_unread_chat_alt,
        SessionState.running => Icons.bolt,
        SessionState.done => Icons.check,
        SessionState.exited => Icons.power_settings_new,
      };

  String label(AppL10n l10n) => switch (this) {
        SessionState.needsInput => l10n.stateNeedsInput,
        SessionState.running => l10n.stateRunning,
        SessionState.done => l10n.stateDone,
        SessionState.exited => l10n.stateExited,
      };
}

/// 8 px status dot. "Đang chạy" breathes between 0.45 and 1 opacity over 1.6 s, the one loop in the app;
/// with Reduce Motion it stays still.
class StatusDot extends StatefulWidget {
  const StatusDot({super.key, required this.state});
  final SessionState state;

  @override
  State<StatusDot> createState() => _StatusDotState();
}

/// One shared clock for every breathing dot: all of them change in the same frame, so any number of running
/// sessions costs the same few frames per second. It runs only while some dot is visible (its tab is shown)
/// and the app is in the foreground, and it skips ticks while the keyboard is sliding.
class BreathClock extends ChangeNotifier with WidgetsBindingObserver {
  BreathClock._();
  static final BreathClock instance = BreathClock._();

  /// One breath is 1.6 s in 8 steps of 200 ms (5 frames a second at most, 0 when nothing is breathing).
  static const Duration step = Duration(milliseconds: 200);
  static const int steps = 8;

  /// With no touch for this long the dots stop breathing and hold still, so an untouched screen draws no frames.
  /// The next touch wakes them.
  static const Duration idleAfter = Duration(seconds: 20);

  Timer? _timer;
  int _phase = 0;
  int _users = 0;
  bool _foreground = true;
  DateTime _lastTouch = DateTime.now();
  DateTime Function() now = DateTime.now;

  int get phase => _phase;

  /// 1.0 -> 0.45 -> 1.0, eased.
  double get alpha {
    final double t = (_phase <= steps / 2 ? _phase : steps - _phase) / (steps / 2);
    return 1 - 0.55 * Curves.easeInOut.transform(t);
  }

  void acquire() {
    if (_users++ == 0) {
      WidgetsBinding.instance.addObserver(this);
      GestureBinding.instance.pointerRouter.addGlobalRoute(_onPointer);
    }
    _lastTouch = now();
    _sync();
  }

  void release() {
    if (--_users <= 0) {
      _users = 0;
      WidgetsBinding.instance.removeObserver(this);
      GestureBinding.instance.pointerRouter.removeGlobalRoute(_onPointer);
    }
    _sync();
  }

  void _onPointer(PointerEvent event) {
    if (event is! PointerDownEvent) return;
    _lastTouch = now();
    if (_timer == null) _sync();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    _foreground = state == AppLifecycleState.resumed;
    _sync();
  }

  void _sync() {
    final bool run = _users > 0 && _foreground;
    if (run && _timer == null) {
      _timer = Timer.periodic(step, (Timer _) {
        if (now().difference(_lastTouch) > idleAfter) {
          // Untouched for a while: rest on a full dot (one last frame) and stop the timer.
          _timer?.cancel();
          _timer = null;
          if (_phase != 0) {
            _phase = 0;
            notifyListeners();
          }
          return;
        }
        final bool keyboardMoving = WidgetsBinding.instance.platformDispatcher.views.any((ui.FlutterView v) => v.viewInsets.bottom > 0);
        if (keyboardMoving) return;
        _phase = (_phase + 1) % steps;
        notifyListeners();
      });
    } else if (!run && _timer != null) {
      _timer!.cancel();
      _timer = null;
    }
  }
}

class _StatusDotState extends State<StatusDot> {
  bool _listening = false;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _sync();
  }

  @override
  void didUpdateWidget(StatusDot oldWidget) {
    super.didUpdateWidget(oldWidget);
    _sync();
  }

  void _sync() {
    // A hidden tab has TickerMode off: its dots hold still and do not ask for frames.
    final bool breathe = widget.state == SessionState.running && !reduceMotion(context) && TickerMode.valuesOf(context).enabled;
    if (breathe && !_listening) {
      BreathClock.instance
        ..addListener(_onTick)
        ..acquire();
      _listening = true;
    } else if (!breathe && _listening) {
      _stop();
    }
  }

  void _stop() {
    BreathClock.instance
      ..removeListener(_onTick)
      ..release();
    _listening = false;
  }

  void _onTick() {
    if (mounted) setState(() {});
  }

  @override
  void dispose() {
    if (_listening) _stop();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final double alpha = _listening ? BreathClock.instance.alpha : 1;
    return RepaintBoundary(
      child: ExcludeSemantics(
        child: SizedBox(
          width: 8,
          height: 8,
          child: DecoratedBox(decoration: BoxDecoration(color: widget.state.color.withValues(alpha: alpha), shape: BoxShape.circle)),
        ),
      ),
    );
  }
}

class InputBadge extends StatelessWidget {
  const InputBadge({super.key});

  @override
  Widget build(BuildContext context) {
    return ExcludeSemantics(
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
        decoration: BoxDecoration(color: Token.statusInput, borderRadius: BorderRadius.circular(Radii.sm)),
        child: Text(AppL10n.of(context).stateNeedsInput, style: TypeToken.labelSm.copyWith(color: Token.onStatusInput)),
      ),
    );
  }
}

String relativeTime(AppL10n l10n, DateTime since, DateTime now) {
  final int seconds = now.difference(since).inSeconds;
  if (seconds < 60) return l10n.justNow;
  if (seconds < 3600) return l10n.minutesShort(seconds ~/ 60);
  if (seconds < 86400) return l10n.hoursShort(seconds ~/ 3600);
  return l10n.daysShort(seconds ~/ 86400);
}
