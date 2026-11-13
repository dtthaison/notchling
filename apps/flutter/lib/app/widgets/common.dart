import 'package:flutter/material.dart';

import '../../l10n/app_localizations.dart';
import '../theme/tokens.dart';
import 'kit.dart';

bool reduceMotion(BuildContext context) =>
    MediaQuery.disableAnimationsOf(context);

class SkeletonRows extends StatefulWidget {
  const SkeletonRows({super.key, this.count = 6});
  final int count;

  @override
  State<SkeletonRows> createState() => _SkeletonRowsState();
}

class _SkeletonRowsState extends State<SkeletonRows>
    with SingleTickerProviderStateMixin {
  late final AnimationController _sweep = AnimationController(
    vsync: this,
    duration: const Duration(milliseconds: 1500),
  );

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final bool run =
        !reduceMotion(context) && TickerMode.valuesOf(context).enabled;
    if (run && !_sweep.isAnimating) {
      _sweep.repeat();
    } else if (!run) {
      _sweep.stop();
    }
  }

  @override
  void dispose() {
    _sweep.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final Kit kit = Kit.of(context);
    Widget bar(double width, double height, {bool round = false}) => Container(
      width: width,
      height: height,
      decoration: BoxDecoration(
        color: kit.raised,
        borderRadius: BorderRadius.circular(round ? Radii.full : Radii.sm),
      ),
    );
    return Semantics(
      label: AppL10n.of(context).connecting,
      child: ExcludeSemantics(
        child: AnimatedBuilder(
          animation: _sweep,
          builder: (BuildContext context, Widget? child) => ShaderMask(
            blendMode: BlendMode.srcATop,
            shaderCallback: (Rect rect) {
              final double x = -1.5 + 3 * _sweep.value;
              return LinearGradient(
                begin: Alignment(x - 0.6, -0.3),
                end: Alignment(x + 0.6, 0.3),
                colors: <Color>[
                  const Color(0x00FFFFFF),
                  kit.highlight.withValues(alpha: kit.isDark ? 0.16 : 0.9),
                  const Color(0x00FFFFFF),
                ],
              ).createShader(rect);
            },
            child: child,
          ),
          child: SingleChildScrollView(
            physics: const NeverScrollableScrollPhysics(),
            child: Column(
              children: <Widget>[
                for (int i = 0; i < widget.count; i++)
                  Padding(
                    padding: const EdgeInsets.symmetric(
                      horizontal: Space.lg,
                      vertical: Space.md,
                    ),
                    child: Row(
                      children: <Widget>[
                        bar(40, 40, round: true),
                        const SizedBox(width: Space.md),
                        Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: <Widget>[
                            bar(140.0 + (i * 37) % 90, 13),
                            const SizedBox(height: 8),
                            bar(90.0 + (i * 23) % 60, 10),
                          ],
                        ),
                        const Spacer(),
                        bar(34, 10),
                      ],
                    ),
                  ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class StateMessage extends StatelessWidget {
  const StateMessage({
    super.key,
    this.icon,
    this.pet,
    required this.title,
    this.body,
    this.action,
    this.isError = false,
  });
  final IconData? icon;

  final String? pet;
  final String title;
  final String? body;
  final Widget? action;
  final bool isError;

  @override
  Widget build(BuildContext context) {
    return Center(
      child: SingleChildScrollView(
        padding: const EdgeInsets.all(Space.xl),
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 420),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: <Widget>[
              if (pet != null)
                KitPet(pose: pet!, size: 96)
              else if (icon != null)
                ExcludeSemantics(
                  child: Icon(
                    icon,
                    size: 28,
                    color: isError
                        ? Kit.of(context).danger
                        : Kit.of(context).textMuted,
                  ),
                ),
              const SizedBox(height: Space.md),
              Text(
                title,
                textAlign: TextAlign.center,
                style: Kit.of(context).titleMd.copyWith(
                  color: isError
                      ? Kit.of(context).danger
                      : Kit.of(context).text,
                ),
              ),
              if (body != null) ...<Widget>[
                const SizedBox(height: Space.sm),
                Text(
                  body!,
                  textAlign: TextAlign.center,
                  style: Kit.of(
                    context,
                  ).bodyMd.copyWith(color: Kit.of(context).textMuted),
                ),
              ],
              if (action != null) ...<Widget>[
                const SizedBox(height: Space.lg),
                action!,
              ],
            ],
          ),
        ),
      ),
    );
  }
}

class ErrorState extends StatelessWidget {
  const ErrorState({super.key, required this.message, required this.onRetry});
  final String message;
  final VoidCallback onRetry;

  @override
  Widget build(BuildContext context) {
    return StateMessage(
      icon: Icons.error_outline,
      title: message,
      isError: true,
      action: SecondaryButton(
        label: AppL10n.of(context).retry,
        onPressed: onRetry,
      ),
    );
  }
}

class PrimaryButton extends StatelessWidget {
  const PrimaryButton({
    super.key,
    required this.label,
    required this.onPressed,
    this.icon,
    this.semanticLabel,
  });
  final String label;
  final VoidCallback? onPressed;
  final IconData? icon;
  final String? semanticLabel;

  @override
  Widget build(BuildContext context) => _PressButton(
    label: label,
    icon: icon,
    semanticLabel: semanticLabel,
    onPressed: onPressed,
    background: Kit.of(context).accent,
    foreground: Kit.of(context).onAccent,
  );
}

class SecondaryButton extends StatelessWidget {
  const SecondaryButton({
    super.key,
    required this.label,
    required this.onPressed,
    this.icon,
    this.semanticLabel,
  });
  final String label;
  final VoidCallback? onPressed;
  final IconData? icon;
  final String? semanticLabel;

  @override
  Widget build(BuildContext context) => _PressButton(
    label: label,
    icon: icon,
    semanticLabel: semanticLabel,
    onPressed: onPressed,
    background: Kit.of(context).card,
    foreground: Kit.of(context).text,
    outlined: true,
  );
}

class _PressButton extends StatefulWidget {
  const _PressButton({
    required this.label,
    required this.onPressed,
    required this.background,
    required this.foreground,
    this.icon,
    this.semanticLabel,
    this.outlined = false,
  });
  final String label;
  final VoidCallback? onPressed;
  final Color background;
  final Color foreground;
  final IconData? icon;
  final String? semanticLabel;
  final bool outlined;

  @override
  State<_PressButton> createState() => _PressButtonState();
}

class _PressButtonState extends State<_PressButton> {
  bool _pressed = false;

  @override
  Widget build(BuildContext context) {
    final bool enabled = widget.onPressed != null;
    final bool still = reduceMotion(context);
    return Semantics(
      button: true,
      enabled: enabled,
      label: widget.semanticLabel ?? widget.label,
      excludeSemantics: true,
      onTap: widget.onPressed,
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTapDown: enabled ? (_) => setState(() => _pressed = true) : null,
        onTapCancel: () => setState(() => _pressed = false),
        onTapUp: (_) => setState(() => _pressed = false),
        onTap: widget.onPressed,
        child: AnimatedScale(
          scale: _pressed && !still ? 0.98 : 1,
          duration: still ? Duration.zero : const Duration(milliseconds: 120),
          child: Opacity(
            opacity: enabled ? 1 : 0.45,
            child: Container(
              constraints: const BoxConstraints(
                minHeight: Metric.buttonHeight,
                minWidth: Metric.minTarget,
              ),
              padding: const EdgeInsets.symmetric(
                horizontal: Space.lg,
                vertical: Space.sm,
              ),
              decoration: BoxDecoration(
                color: widget.background,
                borderRadius: BorderRadius.circular(Radii.sm),
                border: widget.outlined
                    ? Border.all(color: Kit.of(context).hairline)
                    : null,
              ),
              child: Row(
                mainAxisSize: MainAxisSize.min,
                mainAxisAlignment: MainAxisAlignment.center,
                children: <Widget>[
                  if (widget.icon != null) ...<Widget>[
                    Icon(widget.icon, size: 16, color: widget.foreground),
                    const SizedBox(width: Space.sm),
                  ],
                  Flexible(
                    child: Text(
                      widget.label,
                      style: Kit.of(context).labelSm.copyWith(
                        color: widget.foreground,
                        fontSize: 13,
                      ),
                      textAlign: TextAlign.center,
                    ),
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class PanelTitle extends StatelessWidget {
  const PanelTitle(this.text, {super.key});
  final String text;

  @override
  Widget build(BuildContext context) => Semantics(
    header: true,
    child: Text(text.toUpperCase(), style: Kit.of(context).panelTitle),
  );
}
