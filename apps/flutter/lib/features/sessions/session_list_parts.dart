import 'package:flutter/material.dart';

import '../../app/theme/tokens.dart';
import '../../app/widgets/motion.dart';
import '../../core/protocol/models.dart';
import '../../l10n/app_localizations.dart';
import 'status_style.dart';

class ProjectChip extends StatelessWidget {
  const ProjectChip({
    super.key,
    required this.label,
    required this.selected,
    required this.onTap,
  });
  final String label;
  final bool selected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final Kit kit = Kit.of(context);
    return Padding(
      padding: const EdgeInsets.only(right: Space.sm),
      child: Semantics(
        button: true,
        selected: selected,
        label: label,
        excludeSemantics: true,
        onTap: onTap,
        child: InkWell(
          borderRadius: BorderRadius.circular(999),
          onTap: onTap,
          child: Container(
            constraints: const BoxConstraints(minHeight: 40, maxWidth: 200),
            padding: const EdgeInsets.symmetric(horizontal: Space.md),
            alignment: Alignment.center,
            decoration: BoxDecoration(
              color: selected ? kit.selection : kit.raised,
              borderRadius: BorderRadius.circular(999),
              border: Border.all(
                color: selected
                    ? kit.accent.withValues(alpha: 0.5)
                    : kit.hairline,
              ),
            ),
            child: Center(
              widthFactor: 1,
              child: Text(
                label,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: kit.label.copyWith(
                  color: selected ? kit.accentText : kit.text,
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class SessionSwipeRow extends StatelessWidget {
  const SessionSwipeRow({
    super.key,
    required this.session,
    required this.child,
    required this.onArchive,
    this.onTakeover,
  });
  final Session session;
  final Widget child;
  final VoidCallback onArchive;
  final VoidCallback? onTakeover;

  @override
  Widget build(BuildContext context) {
    final Kit kit = Kit.of(context);
    final AppL10n l10n = AppL10n.of(context);
    return Dismissible(
      key: ValueKey<String>('swipe-${session.id}'),
      direction: onTakeover == null
          ? DismissDirection.endToStart
          : DismissDirection.horizontal,
      dismissThresholds: const <DismissDirection, double>{
        DismissDirection.startToEnd: 0.35,
        DismissDirection.endToStart: 0.35,
      },
      confirmDismiss: (DismissDirection direction) async {
        if (direction == DismissDirection.startToEnd) {
          onTakeover?.call();
          return false;
        }
        return true;
      },
      onDismissed: (DismissDirection direction) => onArchive(),
      background: _SwipeBackground(
        color: kit.accent,
        foreground: kit.onAccent,
        icon: Icons.login,
        label: l10n.sessionsTakeoverShort,
        alignment: Alignment.centerLeft,
      ),
      secondaryBackground: _SwipeBackground(
        color: kit.raised,
        foreground: kit.text,
        icon: Icons.archive_outlined,
        label: l10n.sessionsArchive,
        alignment: Alignment.centerRight,
      ),
      child: ColoredBox(color: kit.window, child: child),
    );
  }
}

class _SwipeBackground extends StatelessWidget {
  const _SwipeBackground({
    required this.color,
    required this.foreground,
    required this.icon,
    required this.label,
    required this.alignment,
  });
  final Color color;
  final Color foreground;
  final IconData icon;
  final String label;
  final Alignment alignment;

  @override
  Widget build(BuildContext context) {
    final Kit kit = Kit.of(context);
    return Container(
      alignment: alignment,
      margin: const EdgeInsets.symmetric(horizontal: Space.sm, vertical: 1),
      padding: const EdgeInsets.symmetric(horizontal: Space.lg),
      decoration: BoxDecoration(
        color: color,
        borderRadius: BorderRadius.circular(Radii.md),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: <Widget>[
          Icon(icon, size: 20, color: foreground),
          const SizedBox(width: Space.sm),
          Text(label, style: kit.label.copyWith(color: foreground)),
        ],
      ),
    );
  }
}

class SessionGroupHeader extends StatelessWidget {
  const SessionGroupHeader({super.key, required this.state, required this.count});
  final SessionState state;
  final int count;

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final Kit kit = Kit.of(context);
    return Semantics(
      header: true,
      label: l10n.sessionsGroupLabel(count, state.label(l10n)),
      excludeSemantics: true,
      child: Padding(
        padding: const EdgeInsets.fromLTRB(
          Space.lg + Space.sm,
          Space.lg,
          Space.lg,
          Space.xs,
        ),
        child: Row(
          children: <Widget>[
            Icon(state.icon, size: 13, color: state.color(kit)),
            const SizedBox(width: 6),
            Text(state.label(l10n).toUpperCase(), style: kit.section),
            const SizedBox(width: Space.sm),
            AnimatedSwitcher(
              duration: motionDuration(context, Motion.sheet),
              transitionBuilder: (Widget child, Animation<double> a) =>
                  FadeTransition(
                    opacity: a,
                    child: SlideTransition(
                      position: Tween<Offset>(
                        begin: const Offset(0, 0.6),
                        end: Offset.zero,
                      ).animate(a),
                      child: child,
                    ),
                  ),
              child: Text(
                '$count',
                key: ValueKey<int>(count),
                style: kit.section,
              ),
            ),
          ],
        ),
      ),
    );
  }
}

