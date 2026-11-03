import 'package:flutter/material.dart';

import '../../l10n/app_localizations.dart';
import '../theme/tokens.dart';

/// True when the user asked the system for less motion.
bool reduceMotion(BuildContext context) => MediaQuery.disableAnimationsOf(context);

/// Loading state: static bars in the footprint of the real rows. Only the running dot loops in this app.
class SkeletonRows extends StatelessWidget {
  const SkeletonRows({super.key, this.count = 6});
  final int count;

  @override
  Widget build(BuildContext context) {
    return Semantics(
      label: AppL10n.of(context).connecting,
      child: ExcludeSemantics(
        child: Column(
          children: <Widget>[
            for (int i = 0; i < count; i++)
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: Space.lg, vertical: Space.md),
                child: Row(
                  children: <Widget>[
                    Container(width: 8, height: 8, decoration: const BoxDecoration(color: Token.surfaceRaised, shape: BoxShape.circle)),
                    const SizedBox(width: Space.md),
                    Container(
                        width: 120.0 + (i * 37) % 90,
                        height: 12,
                        decoration: BoxDecoration(color: Token.surfaceRaised, borderRadius: BorderRadius.circular(Radii.sm))),
                    const Spacer(),
                    Container(width: 36, height: 10, decoration: BoxDecoration(color: Token.surfaceRaised, borderRadius: BorderRadius.circular(Radii.sm))),
                  ],
                ),
              ),
          ],
        ),
      ),
    );
  }
}

/// A centred message with an icon, optional body and optional action.
class StateMessage extends StatelessWidget {
  const StateMessage({super.key, required this.icon, required this.title, this.body, this.action, this.isError = false});
  final IconData icon;
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
              ExcludeSemantics(child: Icon(icon, size: 28, color: isError ? Token.statusError : Token.muted)),
              const SizedBox(height: Space.md),
              Text(title, textAlign: TextAlign.center, style: TypeToken.titleMd.copyWith(color: isError ? Token.statusError : Token.onSurface)),
              if (body != null) ...<Widget>[
                const SizedBox(height: Space.sm),
                Text(body!, textAlign: TextAlign.center, style: TypeToken.bodyMd.copyWith(color: Token.muted)),
              ],
              if (action != null) ...<Widget>[const SizedBox(height: Space.lg), action!],
            ],
          ),
        ),
      ),
    );
  }
}

/// Error with the plain-language sentence and a Thử lại button.
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
      action: SecondaryButton(label: AppL10n.of(context).retry, onPressed: onRetry),
    );
  }
}

/// The one primary button of a screen: `primary` fill, `on-primary` text, 48 dp tall, presses to 0.98.
class PrimaryButton extends StatelessWidget {
  const PrimaryButton({super.key, required this.label, required this.onPressed, this.icon, this.semanticLabel});
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
        background: Token.primary,
        foreground: Token.onPrimary,
      );
}

class SecondaryButton extends StatelessWidget {
  const SecondaryButton({super.key, required this.label, required this.onPressed, this.icon, this.semanticLabel});
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
        background: Token.surfaceRaised,
        foreground: Token.onSurface,
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
              constraints: const BoxConstraints(minHeight: Metric.buttonHeight, minWidth: Metric.minTarget),
              padding: const EdgeInsets.symmetric(horizontal: Space.lg, vertical: Space.sm),
              decoration: BoxDecoration(
                color: widget.background,
                borderRadius: BorderRadius.circular(Radii.md),
                border: widget.outlined ? Border.all(color: Token.border) : null,
              ),
              child: Row(
                mainAxisSize: MainAxisSize.min,
                mainAxisAlignment: MainAxisAlignment.center,
                children: <Widget>[
                  if (widget.icon != null) ...<Widget>[Icon(widget.icon, size: 16, color: widget.foreground), const SizedBox(width: Space.sm)],
                  Flexible(child: Text(widget.label, style: TypeToken.labelSm.copyWith(color: widget.foreground, fontSize: 13), textAlign: TextAlign.center)),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}

/// 11 px uppercase section title.
class PanelTitle extends StatelessWidget {
  const PanelTitle(this.text, {super.key});
  final String text;

  @override
  Widget build(BuildContext context) => Semantics(header: true, child: Text(text.toUpperCase(), style: TypeToken.panelTitle));
}
