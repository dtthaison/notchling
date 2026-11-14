import 'dart:async';
import 'dart:ui' show ImageFilter;

import 'package:flutter/material.dart';

import '../theme/tokens.dart';
import 'motion.dart';

/// Parts of the Notchling Chat kit that both screens use: avatar, status pill, Claude's mark, meters and the pet.

const List<(Color, Color)> _avatarPairs = <(Color, Color)>[
  (Color(0xFFF7873B), Color(0xFFE0533A)),
  (Color(0xFF7DCB2C), Color(0xFF2E9E5B)),
  (Color(0xFF6EB0FF), Color(0xFF4A62E8)),
  (Color(0xFFC48BFF), Color(0xFF8A4FE0)),
  (Color(0xFFFFC94D), Color(0xFFF0913A)),
  (Color(0xFF4FD1C5), Color(0xFF2B8FB3)),
  (Color(0xFFFF8FB1), Color(0xFFD9507E)),
  (Color(0xFFB7A68F), Color(0xFF7D6B55)),
];

/// A session keeps its colors: FNV-1a of its name, high bits, like the Mac.
(Color, Color) avatarColors(String name) {
  int hash = 0xcbf29ce484222325;
  for (final int byte in name.codeUnits.expand(
    (int unit) =>
        unit < 128 ? <int>[unit] : String.fromCharCode(unit).codeUnits,
  )) {
    hash = ((hash ^ byte) * 0x100000001b3) & 0xFFFFFFFFFFFFFFFF;
  }
  return _avatarPairs[(hash >>> 40) % _avatarPairs.length];
}

String initialsOf(String name) {
  final Iterable<String> words = name
      .split(RegExp(r'[^\p{L}\p{N}]+', unicode: true))
      .where((String w) => w.isNotEmpty);
  final String letters = words
      .take(2)
      .map((String w) => String.fromCharCodes(w.runes.take(1)))
      .join();
  return letters.isEmpty ? '?' : letters.toUpperCase();
}

class KitAvatar extends StatelessWidget {
  const KitAvatar({
    super.key,
    required this.name,
    this.size = 40,
    this.status,
    this.badge,
  });
  final String name;
  final double size;

  /// The status color for the badge in the corner, if any.
  final Color? status;

  /// A widget for the corner instead of the plain dot (the animated status badge).
  final Widget? badge;

  @override
  Widget build(BuildContext context) {
    final Kit kit = Kit.of(context);
    final (Color, Color) pair = avatarColors(name);
    return ExcludeSemantics(
      child: SizedBox(
        width: size,
        height: size,
        child: Stack(
          clipBehavior: Clip.none,
          children: <Widget>[
            Container(
              width: size,
              height: size,
              alignment: Alignment.center,
              decoration: BoxDecoration(
                shape: BoxShape.circle,
                // One light, from above: a lighter top over the session's own colour, no glow.
                gradient: LinearGradient(
                  begin: Alignment.topCenter,
                  end: Alignment.bottomCenter,
                  colors: <Color>[
                    Color.lerp(pair.$1, const Color(0xFFFFFFFF), 0.22)!,
                    pair.$2,
                  ],
                ),
              ),
              child: Text(
                initialsOf(name),
                textScaler: TextScaler.noScaling,
                style: TextStyle(
                  fontSize: size * 0.4,
                  fontWeight: FontWeight.w700,
                  color: const Color(0xFFFFFFFF),
                  height: 1,
                ),
              ),
            ),
            if (badge != null) Positioned(right: -3, bottom: -3, child: badge!),
            if (status != null && badge == null)
              Positioned(
                right: -2,
                bottom: -2,
                child: Container(
                  width: size * 0.34,
                  height: size * 0.34,
                  decoration: BoxDecoration(
                    color: status,
                    shape: BoxShape.circle,
                    border: Border.all(color: kit.window, width: 2),
                  ),
                ),
              ),
          ],
        ),
      ),
    );
  }
}

/// Claude's mark: a sparkle on an orange disc.
class KitClaudeMark extends StatelessWidget {
  const KitClaudeMark({super.key, this.size = 24});
  final double size;

  @override
  Widget build(BuildContext context) {
    final Kit kit = Kit.of(context);
    return ExcludeSemantics(
      child: Container(
        width: size,
        height: size,
        decoration: BoxDecoration(
          shape: BoxShape.circle,
          gradient: LinearGradient(
            begin: Alignment.topLeft,
            end: Alignment.bottomRight,
            colors: <Color>[const Color(0xFFFFB47E), kit.accent],
          ),
        ),
        child: Icon(Icons.auto_awesome, size: size * 0.55, color: kit.onAccent),
      ),
    );
  }
}

/// Status as a capsule: icon and word, never color alone.
class KitStatusPill extends StatelessWidget {
  const KitStatusPill({
    super.key,
    required this.color,
    required this.icon,
    required this.text,
    this.accentText = false,
  });
  final Color color;
  final IconData icon;
  final String text;

  /// The needs-input orange is not readable as text on every surface; words use the accent-text color.
  final bool accentText;

  @override
  Widget build(BuildContext context) {
    final Kit kit = Kit.of(context);
    final Color word = accentText ? kit.accentText : color;
    return Container(
      constraints: const BoxConstraints(minHeight: 24),
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
      decoration: BoxDecoration(
        color: color.withValues(alpha: 0.16),
        borderRadius: BorderRadius.circular(999),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: <Widget>[
          Icon(icon, size: 12, color: word),
          const SizedBox(width: 4),
          Flexible(
            child: Text(
              text,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: kit.label.copyWith(color: word, fontSize: 12.5),
            ),
          ),
        ],
      ),
    );
  }
}

/// A label, a value and a 3 dp bar (as wide as its text, at least [width]): context in the chat header, the account's limits. The bar turns orange from 80%.
class KitMeter extends StatelessWidget {
  const KitMeter({
    super.key,
    required this.label,
    required this.value,
    this.fraction,
    this.width,
    this.icon,
  });
  final String label;
  final String value;
  final double? fraction;
  final double? width;

  /// Shown in place of the label when there is no room for the word (large text).
  final IconData? icon;

  @override
  Widget build(BuildContext context) {
    final Kit kit = Kit.of(context);
    final double? f = fraction?.clamp(0, 1).toDouble();
    return Semantics(
      label: '$label $value',
      excludeSemantics: true,
      child: ConstrainedBox(
        constraints: BoxConstraints(minWidth: width ?? 0),
        child: IntrinsicWidth(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            mainAxisSize: MainAxisSize.min,
            children: <Widget>[
              Text.rich(
                TextSpan(
                  children: <InlineSpan>[
                    if (icon != null)
                      WidgetSpan(
                        alignment: PlaceholderAlignment.middle,
                        child: Padding(
                          padding: const EdgeInsets.only(right: 3),
                          child: Icon(icon, size: 13, color: kit.textMuted),
                        ),
                      )
                    else
                      TextSpan(text: '$label ', style: kit.meta),
                    TextSpan(text: value, style: kit.label),
                  ],
                ),
                maxLines: 1,
              ),
              if (f != null) ...<Widget>[
                const SizedBox(height: 3),
                // The fill is a full-width bar scaled from the left (transform only), so a new value glides there
                // instead of jumping; orange from 80%, red from 95%. (It used to be a FractionallySizedBox around
                // a child-less ColoredBox, which is 0 tall: only the track showed.)
                TweenAnimationBuilder<double>(
                  tween: Tween<double>(end: f < 0.02 ? 0.02 : f),
                  duration: motionDuration(
                    context,
                    const Duration(milliseconds: 420),
                  ),
                  curve: Motion.out,
                  builder: (BuildContext context, double value, Widget? _) =>
                      ClipRRect(
                        borderRadius: BorderRadius.circular(2),
                        child: SizedBox(
                          height: 3,
                          child: Stack(
                            children: <Widget>[
                              Positioned.fill(
                                child: ColoredBox(color: kit.raised),
                              ),
                              Positioned.fill(
                                child: Transform(
                                  key: const ValueKey<String>('meter-fill'),
                                  alignment: Alignment.centerLeft,
                                  transform: Matrix4.diagonal3Values(
                                    value,
                                    1,
                                    1,
                                  ),
                                  child: ColoredBox(
                                    color: f >= 0.95
                                        ? kit.danger
                                        : (f >= 0.8 ? kit.needs : kit.working),
                                  ),
                                ),
                              ),
                            ],
                          ),
                        ),
                      ),
                ),
              ],
            ],
          ),
        ),
      ),
    );
  }
}

/// The pet from the notch for empty states. It enters with a fade and a small settle, glances to the side every few
/// seconds for half a minute, then falls asleep and stays still: a gentle idle, not a loop. A missing drawing falls
/// back to Claude's mark. With Reduce Motion it holds one pose.
class KitPet extends StatefulWidget {
  const KitPet({
    super.key,
    this.pose = 'sit-1',
    this.size = 96,
    this.idle = true,
  });
  final String pose;
  final double size;
  final bool idle;

  @override
  State<KitPet> createState() => _KitPetState();
}

class _KitPetState extends State<KitPet> {
  Timer? _timer;
  late String _pose = widget.pose;
  int _ticks = 0;
  bool _started = false;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    if (_started || !widget.idle || MediaQuery.disableAnimationsOf(context)) {
      return;
    }
    _started = true;
    _timer = Timer.periodic(const Duration(seconds: 4), (Timer _) {
      if (!mounted || !TickerMode.valuesOf(context).enabled) return;
      _ticks++;
      setState(
        () => _pose = _ticks >= 14
            ? 'sleepy-1'
            : (_ticks % 4 == 3 ? 'hop-1' : (_ticks.isOdd ? 'look-2' : widget.pose)),
      );
      if (_ticks >= 14) _timer?.cancel();
    });
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final Widget image = AnimatedSwitcher(
      duration: motionDuration(context),
      child: Image.asset(
        'assets/pet/$_pose.png',
        key: ValueKey<String>(_pose),
        width: widget.size,
        height: widget.size,
        fit: BoxFit.contain,
        filterQuality: FilterQuality.medium,
        errorBuilder: (BuildContext context, Object error, StackTrace? stack) =>
            KitClaudeMark(size: widget.size / 2),
      ),
    );
    return ExcludeSemantics(
      child: RepaintBoundary(child: Entrance(rise: 8, child: image)),
    );
  }
}

/// A rounded fill with a hairline, for cards and bubbles.
BoxDecoration kitCard(
  Kit kit, {
  double radius = Radii.card,
  Color? fill,
  Color? border,
}) => BoxDecoration(
  color: fill ?? kit.card,
  borderRadius: BorderRadius.circular(radius),
  border: Border.all(color: border ?? kit.hairline),
);

/// The surface of a card, bubble, capsule or sheet, lit from one direction (above): a fill, a 1 px edge that is
/// light at the top and a little darker at the bottom (no grey outline all around), and a short dark shadow.
/// [level] 0 is flat (the window), 1 is cards, rows and Claude's bubbles, 2 is the composer, the permission card,
/// sheets and banners. The corner shape is a superellipse (smooth, not a plain arc).
class Surface extends StatelessWidget {
  const Surface({
    super.key,
    required this.child,
    this.level = 1,
    this.radius,
    this.fill,
    this.padding,
    this.outline,
    this.ring,
    this.clip = false,
  });

  final Widget child;
  final int level;
  final BorderRadius? radius;
  final Color? fill;
  final EdgeInsetsGeometry? padding;

  /// A full outline in this colour (the permission card's orange), instead of only the lit edge.
  final Color? outline;

  /// A 2 px ring outside the shape (focus), in this colour.
  final Color? ring;

  /// Clip the child to the shape (images, scrolling code).
  final bool clip;

  @override
  Widget build(BuildContext context) {
    final Kit kit = Kit.of(context);
    final BorderRadius r = radius ?? BorderRadius.circular(Radii.md);
    final RoundedSuperellipseBorder shape = RoundedSuperellipseBorder(
      borderRadius: r,
      side: outline == null ? BorderSide.none : BorderSide(color: outline!),
    );
    final List<BoxShadow> shadows = <BoxShadow>[
      if (level == 1) ...kit.shadow1,
      if (level >= 2) ...kit.shadow2,
      if (ring != null) BoxShadow(color: ring!, spreadRadius: 2),
    ];
    Widget content = padding == null
        ? child
        : Padding(padding: padding!, child: child);
    if (clip) {
      content = ClipPath(
        clipper: ShapeBorderClipper(shape: shape),
        child: content,
      );
    }
    return RepaintBoundary(
      child: CustomPaint(
        foregroundPainter: outline == null ? _EdgePainter(shape, kit) : null,
        // The shadow is cast by a plain rounded rectangle: the GPU blurs that shape analytically, while a blurred
        // superellipse path is a full Gaussian pass every frame the surface moves (the composer while the keyboard
        // slides, bubbles while scrolling). Under the fill the difference doesn't show.
        child: DecoratedBox(
          decoration: BoxDecoration(borderRadius: r, boxShadow: shadows.isEmpty ? null : shadows),
          child: DecoratedBox(
            decoration: ShapeDecoration(color: fill ?? kit.card, shape: shape),
            child: content,
          ),
        ),
      ),
    );
  }
}

class _EdgePainter extends CustomPainter {
  const _EdgePainter(this.shape, this.kit);
  final RoundedSuperellipseBorder shape;
  final Kit kit;

  @override
  void paint(Canvas canvas, Size size) {
    final Rect rect = (Offset.zero & size).deflate(0.5);
    if (rect.isEmpty) return;
    final Paint paint = Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = 1
      ..shader = LinearGradient(
        begin: Alignment.topCenter,
        end: Alignment.bottomCenter,
        colors: <Color>[
          kit.highlight,
          kit.highlight.withValues(alpha: 0),
          kit.edge.withValues(alpha: 0),
          kit.edge,
        ],
        stops: const <double>[0, 0.35, 0.7, 1],
      ).createShader(rect);
    canvas.drawPath(shape.getOuterPath(rect), paint);
  }

  @override
  bool shouldRepaint(_EdgePainter oldDelegate) =>
      oldDelegate.kit != kit || oldDelegate.shape != shape;
}

/// A frosted bar: the content that scrolls under it is blurred (one BackdropFilter, in its own repaint boundary) and
/// tinted by [Kit.glass]; a single hairline marks the edge facing the content.
class GlassBar extends StatelessWidget {
  const GlassBar({super.key, required this.child, required this.top, this.solid = false});
  final Widget child;

  /// An opaque fill instead of a blur. The Chat tab uses it: its list moves under the bar while the keyboard slides,
  /// and a BackdropFilter would re-blur the moving content every frame. Any bar also turns solid while the keyboard
  /// is up, for the same reason.
  final bool solid;

  /// True for the top bar (edge line at the bottom), false for the bottom bar (edge line at the top).
  final bool top;

  @override
  Widget build(BuildContext context) {
    final Kit kit = Kit.of(context);
    final BorderSide line = BorderSide(
      color: kit.hairline,
      width: 1 / MediaQuery.devicePixelRatioOf(context),
    );
    final Border border = Border(
      top: top ? BorderSide.none : line,
      bottom: top ? line : BorderSide.none,
    );
    if (solid || MediaQuery.viewInsetsOf(context).bottom > 0) {
      return RepaintBoundary(
        child: DecoratedBox(
          decoration: BoxDecoration(color: Color.alphaBlend(kit.glass, kit.window), border: border),
          child: child,
        ),
      );
    }
    return RepaintBoundary(
      child: ClipRect(
        child: BackdropFilter(
          filter: ImageFilter.blur(sigmaX: 12, sigmaY: 12),
          child: DecoratedBox(
            decoration: BoxDecoration(color: kit.glass, border: border),
            child: child,
          ),
        ),
      ),
    );
  }
}
