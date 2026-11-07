import 'dart:async';
import 'dart:ui' as ui;

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';

import '../../app/theme/tokens.dart';
import '../../app/widgets/common.dart';
import '../../app/widgets/kit.dart';
import '../../app/widgets/motion.dart';
import '../../core/protocol/models.dart';
import '../../l10n/app_localizations.dart';

extension SessionStateStyle on SessionState {
  /// Needs Input orange, Working blue, Done lime; an ended session is faint.
  Color color(Kit kit) => switch (this) {
    SessionState.needsInput => kit.needs,
    SessionState.running => kit.working,
    SessionState.done => kit.done,
    SessionState.exited => kit.textFaint,
  };

  /// Words in the status color where it reads (blue and lime on the kit surfaces; orange is the accent-text tone).
  Color labelColor(Kit kit) => switch (this) {
    SessionState.needsInput => kit.accentText,
    SessionState.running => kit.working,
    SessionState.done => kit.brightness == Brightness.dark ? kit.done : kit.add,
    SessionState.exited => kit.textMuted,
  };

  IconData get icon => switch (this) {
    SessionState.needsInput => Icons.mark_chat_unread_outlined,
    SessionState.running => Icons.motion_photos_on_outlined,
    SessionState.done => Icons.check_circle,
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
  const StatusDot({
    super.key,
    required this.state,
    this.size = 8,
    this.ring = false,
  });
  final SessionState state;

  /// 8 in lists; 14 as the badge on an avatar, with a ring in the window color around it.
  final double size;
  final bool ring;

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
    final double t =
        (_phase <= steps / 2 ? _phase : steps - _phase) / (steps / 2);
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
        final bool keyboardMoving = WidgetsBinding
            .instance
            .platformDispatcher
            .views
            .any((ui.FlutterView v) => v.viewInsets.bottom > 0);
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
    final bool breathe =
        (widget.state == SessionState.running || widget.state == SessionState.needsInput) &&
        !reduceMotion(context) &&
        // Still while the keyboard moves the page: one less thing to repaint per frame.
        MediaQuery.viewInsetsOf(context).bottom == 0 &&
        TickerMode.valuesOf(context).enabled;
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
    final Kit kit = Kit.of(context);
    final bool needs = widget.state == SessionState.needsInput;
    final double breath = _listening ? BreathClock.instance.alpha : 1;
    // Working breathes (opacity). Needs Input does not blink: it bounces once when it appears, then a faint ring
    // swells and fades slowly around it.
    final double alpha = needs ? 1 : breath;
    final double ring = needs && _listening ? (1 - breath) * 0.9 : 0;
    Widget dot = TweenAnimationBuilder<Color?>(
      tween: ColorTween(end: widget.state.color(kit)),
      duration: motionDuration(context, const Duration(milliseconds: 250)),
      builder: (BuildContext context, Color? color, Widget? _) => SizedBox(
        width: widget.size,
        height: widget.size,
        child: DecoratedBox(
          decoration: BoxDecoration(
            color: (color ?? widget.state.color(kit)).withValues(alpha: alpha),
            shape: BoxShape.circle,
            border: widget.ring ? Border.all(color: kit.window, width: 2) : null,
            boxShadow: ring > 0.02 ? <BoxShadow>[BoxShadow(color: kit.needs.withValues(alpha: ring), spreadRadius: 2.5 * (1 - breath) * 2)] : null,
          ),
        ),
      ),
    );
    if (needs && !reduceMotion(context)) {
      // Keyed by the state, so it plays each time a session turns to Needs Input.
      dot = TweenAnimationBuilder<double>(
        key: const ValueKey<String>('needs-bounce'),
        tween: Tween<double>(begin: 0.3, end: 1),
        duration: const Duration(milliseconds: 520),
        curve: Curves.easeOutBack,
        builder: (BuildContext context, double scale, Widget? child) => Transform.scale(scale: scale, child: child),
        child: dot,
      );
    }
    return RepaintBoundary(child: ExcludeSemantics(child: dot));
  }
}

/// The state as icon and word, crossfading to the new one when the state changes.
class StatePill extends StatelessWidget {
  const StatePill({super.key, required this.state});
  final SessionState state;

  @override
  Widget build(BuildContext context) {
    final Kit kit = Kit.of(context);
    return AnimatedSwitcher(
      duration: motionDuration(context),
      switchInCurve: Motion.out,
      layoutBuilder: (Widget? current, List<Widget> previous) => Stack(
        alignment: Alignment.centerLeft,
        children: <Widget>[...previous, ?current],
      ),
      child: KitStatusPill(
        key: ValueKey<SessionState>(state),
        color: state.color(kit),
        icon: state.icon,
        text: state.label(AppL10n.of(context)),
        accentText: state == SessionState.needsInput,
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
