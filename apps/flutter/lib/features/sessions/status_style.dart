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
  Color color(Kit kit) => switch (this) {
    SessionState.needsInput => kit.needs,
    SessionState.running => kit.working,
    SessionState.done => kit.done,
    SessionState.exited => kit.textFaint,
  };

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
    SessionState.needsInput => l10n.sessionsStateNeedsInput,
    SessionState.running => l10n.stateRunning,
    SessionState.done => l10n.stateDone,
    SessionState.exited => l10n.sessionsStateExited,
  };
}

class StatusDot extends StatefulWidget {
  const StatusDot({
    super.key,
    required this.state,
    this.size = 8,
    this.ring = false,
  });
  final SessionState state;

  final double size;
  final bool ring;

  @override
  State<StatusDot> createState() => _StatusDotState();
}

class BreathClock extends ChangeNotifier with WidgetsBindingObserver {
  BreathClock._();
  static final BreathClock instance = BreathClock._();

  static const Duration step = Duration(milliseconds: 200);
  static const int steps = 8;

  static const Duration idleAfter = Duration(seconds: 20);

  Timer? _timer;
  int _phase = 0;
  int _users = 0;
  bool _foreground = true;
  DateTime _lastTouch = DateTime.now();
  DateTime Function() now = DateTime.now;

  int get phase => _phase;

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
    final bool breathe =
        (widget.state == SessionState.running || widget.state == SessionState.needsInput) &&
        !reduceMotion(context) &&
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
  if (seconds < 60) return l10n.sessionsJustNow;
  if (seconds < 3600) return l10n.sessionsMinutesShort(seconds ~/ 60);
  if (seconds < 86400) return l10n.sessionsHoursShort(seconds ~/ 3600);
  return l10n.sessionsDaysShort(seconds ~/ 86400);
}
