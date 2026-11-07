import 'package:flutter/material.dart';
import 'package:flutter/physics.dart';
import 'package:flutter/scheduler.dart';

/// Motion tier: calm (DESIGN.md). Fades and color changes use short timing with an ease-out curve; moves are
/// a few dp with no overshoot; nothing starts at scale 0. Everything here honours Reduce Motion by dropping to zero
/// duration, so a state change is still visible (color, shape, text) without movement.
abstract final class Motion {
  /// Springs of the lively tier: UI (rows, bubbles, cards) settles in ~350 ms with a little overshoot; sheets are softer.
  static const SpringDescription springUi = SpringDescription(mass: 1, stiffness: 380, damping: 30);
  static const SpringDescription springSheet = SpringDescription(mass: 1, stiffness: 260, damping: 26);

  static const Duration quick = Duration(milliseconds: 120);
  static const Duration ui = Duration(milliseconds: 200);
  static const Duration sheet = Duration(milliseconds: 280);
  static const Curve out = Curves.easeOutCubic;
  static const Curve inOut = Curves.easeInOutCubic;
}

/// [d], or zero when the user asked the system for less motion.
Duration motionDuration(BuildContext context, [Duration d = Motion.ui]) =>
    MediaQuery.disableAnimationsOf(context) ? Duration.zero : d;

/// A child that grows and fades in when [expanded] turns true and shrinks and fades out when it turns false. Height
/// is animated with AnimatedSize (one layout per frame while it runs, none at rest); opacity is a layer property.
class ExpandReveal extends StatelessWidget {
  const ExpandReveal({
    super.key,
    required this.expanded,
    required this.child,
    this.alignment = Alignment.topCenter,
    this.duration = Motion.sheet,
  });
  final bool expanded;
  final Widget child;
  final Alignment alignment;
  final Duration duration;

  @override
  Widget build(BuildContext context) {
    final Duration d = motionDuration(context, duration);
    // AnimatedSize with a zero duration asserts (it re-dirties itself in layout): with Reduce Motion there is none.
    if (d == Duration.zero) return expanded ? child : const SizedBox(width: double.infinity);
    return AnimatedSize(
      duration: d,
      curve: Motion.out,
      alignment: alignment,
      child: AnimatedSwitcher(
        duration: d,
        switchInCurve: Motion.out,
        switchOutCurve: Curves.easeIn,
        transitionBuilder: (Widget child, Animation<double> animation) =>
            FadeTransition(opacity: animation, child: child),
        layoutBuilder: (Widget? current, List<Widget> previous) => Stack(
          alignment: alignment,
          clipBehavior: Clip.hardEdge,
          children: <Widget>[...previous, ?current],
        ),
        child: expanded
            ? KeyedSubtree(key: const ValueKey<bool>(true), child: child)
            : const SizedBox(
                key: ValueKey<bool>(false),
                width: double.infinity,
              ),
      ),
    );
  }
}

/// Arrives on its first build: fades in while it springs from [offset] (dp) and a slight scale (0.97 to 1) into place,
/// once. Opacity, translation and scale only (no layout). Reduce Motion keeps only a short fade.
class Entrance extends StatefulWidget {
  const Entrance({super.key, required this.child, this.rise = 12, this.offset, this.scale = 0.97, this.delay = Duration.zero, this.spring = Motion.springUi});
  final Widget child;

  /// How far below it starts (dp), when [offset] is not given.
  final double rise;

  /// Where it starts relative to its place (dp), e.g. from the right: Offset(24, 12).
  final Offset? offset;
  final double scale;
  final Duration delay;
  final SpringDescription spring;

  @override
  State<Entrance> createState() => _EntranceState();
}

class _EntranceState extends State<Entrance> with SingleTickerProviderStateMixin {
  // Unbounded, so the spring may overshoot a little before it settles at 1.
  late final AnimationController _controller = AnimationController.unbounded(vsync: this, value: 0);
  bool _started = false;
  bool _still = false;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    if (_started) return;
    _started = true;
    _still = MediaQuery.disableAnimationsOf(context);
    // The delay is part of the simulation (it holds at 0), not a Timer: nothing is left pending if the widget goes away.
    if (_still) {
      _controller.animateTo(1, duration: Motion.quick, curve: Curves.easeOut);
    } else {
      _controller.animateWith(_DelayedSpring(widget.spring, widget.delay));
    }
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final Offset from = widget.offset ?? Offset(0, widget.rise);
    return AnimatedBuilder(
      animation: _controller,
      child: widget.child,
      builder: (BuildContext context, Widget? child) {
        final double t = _controller.value;
        final double opacity = (t * 1.6).clamp(0.0, 1.0);
        if (_still) return Opacity(opacity: opacity, child: child);
        return Opacity(
          opacity: opacity,
          child: Transform.translate(
            offset: Offset(from.dx * (1 - t), from.dy * (1 - t)),
            child: Transform.scale(scale: widget.scale + (1 - widget.scale) * t, child: child),
          ),
        );
      },
    );
  }
}

/// A button face that settles to 0.96 while pressed and back, with the tap going through. Transform only.
class PressScale extends StatefulWidget {
  const PressScale({
    super.key,
    required this.child,
    this.enabled = true,
    this.scale = 0.94,
  });
  final Widget child;
  final bool enabled;
  final double scale;

  @override
  State<PressScale> createState() => _PressScaleState();
}

class _PressScaleState extends State<PressScale> {
  bool _down = false;

  void _set(bool down) {
    if (widget.enabled && _down != down) setState(() => _down = down);
  }

  @override
  Widget build(BuildContext context) {
    return Listener(
      onPointerDown: (_) => _set(true),
      onPointerUp: (_) => _set(false),
      onPointerCancel: (_) => _set(false),
      child: AnimatedScale(
        scale: _down && !MediaQuery.disableAnimationsOf(context)
            ? widget.scale
            : 1,
        duration: motionDuration(context, Motion.quick),
        curve: Motion.out,
        child: widget.child,
      ),
    );
  }
}

/// When the row is laid out somewhere else after its data changed (a session moved to another group, the list above
/// it grew), it starts at its old place and glides to the new one: transform only, 280 ms, no overshoot. Rows that did
/// not move do nothing. Scrolling does not count as a move: positions are measured in the scrollable's content space.
class FlipOnMove extends StatefulWidget {
  const FlipOnMove({super.key, required this.child});
  final Widget child;

  @override
  State<FlipOnMove> createState() => _FlipOnMoveState();
}

class _FlipOnMoveState extends State<FlipOnMove>
    with SingleTickerProviderStateMixin {
  late final AnimationController _controller = AnimationController(
    vsync: this,
    duration: Motion.sheet,
  );
  late final CurvedAnimation _curve = CurvedAnimation(
    parent: _controller,
    curve: Motion.out,
  );
  double? _y;
  double _from = 0;

  double? _contentY() {
    final RenderObject? box = context.findRenderObject();
    final ScrollableState? scrollable = Scrollable.maybeOf(context);
    if (box is! RenderBox ||
        !box.attached ||
        !box.hasSize ||
        scrollable == null) {
      return null;
    }
    final RenderObject? viewport = scrollable.context.findRenderObject();
    if (viewport == null) return null;
    return box.localToGlobal(Offset.zero, ancestor: viewport).dy +
        scrollable.position.pixels;
  }

  void _measure({required bool animate}) {
    final double? now = _contentY();
    if (now == null) return;
    final double? before = _y;
    _y = now;
    if (animate &&
        before != null &&
        (before - now).abs() > 1 &&
        !MediaQuery.disableAnimationsOf(context)) {
      // Where it is drawn now minus where it was: starts there, ends at zero.
      _from = before - now;
      _controller.forward(from: 0);
    }
  }

  @override
  void initState() {
    super.initState();
    SchedulerBinding.instance.addPostFrameCallback((Duration _) {
      if (mounted) _measure(animate: false);
    });
  }

  @override
  void didUpdateWidget(FlipOnMove oldWidget) {
    super.didUpdateWidget(oldWidget);
    SchedulerBinding.instance.addPostFrameCallback((Duration _) {
      if (mounted) _measure(animate: true);
    });
  }

  @override
  void dispose() {
    _curve.dispose();
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return AnimatedBuilder(
      animation: _curve,
      child: widget.child,
      builder: (BuildContext context, Widget? child) => Transform.translate(
        offset: Offset(0, _from * (1 - _curve.value)),
        child: child,
      ),
    );
  }
}

/// AnimatedSize that is simply absent under Reduce Motion (a zero-duration AnimatedSize asserts in layout).
class SmoothSize extends StatelessWidget {
  const SmoothSize({super.key, required this.child, this.alignment = Alignment.topCenter, this.duration = Motion.sheet});
  final Widget child;
  final Alignment alignment;
  final Duration duration;

  @override
  Widget build(BuildContext context) {
    final Duration d = motionDuration(context, duration);
    if (d == Duration.zero) return child;
    return AnimatedSize(duration: d, curve: Motion.out, alignment: alignment, child: child);
  }
}

/// 0 for [delay], then a spring from 0 to 1.
class _DelayedSpring extends Simulation {
  _DelayedSpring(SpringDescription spring, this.delay) : _spring = SpringSimulation(spring, 0, 1, 0);
  final SpringSimulation _spring;
  final Duration delay;

  double get _seconds => delay.inMicroseconds / Duration.microsecondsPerSecond;

  @override
  double x(double time) => time <= _seconds ? 0 : _spring.x(time - _seconds);

  @override
  double dx(double time) => time <= _seconds ? 0 : _spring.dx(time - _seconds);

  @override
  bool isDone(double time) => time > _seconds && _spring.isDone(time - _seconds);
}
