import 'package:flutter/material.dart';

import '../../app/theme/tokens.dart';
import '../../app/widgets/kit.dart';
import '../../app/widgets/motion.dart';
import '../../core/protocol/models.dart';
import '../../l10n/app_localizations.dart';
import 'status_style.dart';

String? activityLine(Session session) {
  if (session.state == SessionState.needsInput && session.pending != null) {
    final PendingPrompt prompt = session.pending!;
    final String what = prompt.isPermission
        ? '${prompt.toolName}: ${prompt.summary}'
        : (prompt.questions.isNotEmpty
              ? prompt.questions.first.question
              : prompt.summary);
    return what.replaceAll('\n', ' ');
  }
  if (session.state == SessionState.running) return session.activity;
  return null;
}

class SessionRow extends StatelessWidget {
  const SessionRow({
    super.key,
    required this.session,
    required this.now,
    required this.onTap,
    this.selected = false,
    this.expanded = false,
    this.onToggleExpand,
    this.onLongPress,
    this.trailing,
  });

  final Session session;
  final DateTime now;
  final bool selected;
  final bool expanded;
  final VoidCallback onTap;
  final VoidCallback? onToggleExpand;
  final VoidCallback? onLongPress;

  final Widget? trailing;

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final Kit kit = Kit.of(context);
    final int running = session.runningSubagents;
    final bool hasSubagents = session.subagents.isNotEmpty;
    final String time = relativeTime(l10n, session.lastActivity, now);
    final String? activity = activityLine(session);
    final bool needs = session.state == SessionState.needsInput;
    final String spoken = <String>[
      session.name,
      session.project,
      session.state.label(l10n),
      ?activity,
      if (running > 0) l10n.sessionsRunningSubagents(running),
      if (session.control == SessionControl.viewOnly) l10n.appViewOnly,
      time,
    ].join(', ');

    return Container(
      margin: const EdgeInsets.symmetric(horizontal: Space.sm, vertical: 1),
      decoration: BoxDecoration(
        color: selected ? kit.selection : null,
        borderRadius: BorderRadius.circular(Radii.md),
        border: selected
            ? Border.all(color: kit.accent.withValues(alpha: 0.35))
            : null,
      ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.center,
        children: <Widget>[
          Expanded(
            child: Semantics(
              button: true,
              selected: selected,
              label: spoken,
              container: true,
              explicitChildNodes: true,
              onTap: onTap,
              onLongPress: onLongPress,
              child: PressScale(
                scale: 0.98,
                child: InkWell(
                  borderRadius: BorderRadius.circular(Radii.md),
                  onTap: onTap,
                  onLongPress: onLongPress,
                  child: ConstrainedBox(
                    constraints: const BoxConstraints(
                      minHeight: Metric.rowHeight,
                    ),
                    child: Padding(
                      padding: const EdgeInsets.fromLTRB(
                        Space.md,
                        Space.sm,
                        Space.md,
                        Space.sm,
                      ),
                      child: Row(
                        crossAxisAlignment: CrossAxisAlignment.center,
                        children: <Widget>[
                          KitAvatar(
                            name: session.name,
                            size: 40,
                            badge: StatusDot(
                              state: session.state,
                              size: 14,
                              ring: true,
                            ),
                          ),
                          const SizedBox(width: Space.md),
                          Expanded(
                            child: Column(
                              crossAxisAlignment: CrossAxisAlignment.stretch,
                              mainAxisSize: MainAxisSize.min,
                              children: <Widget>[
                                ExcludeSemantics(
                                  child: Row(
                                    crossAxisAlignment:
                                        CrossAxisAlignment.baseline,
                                    textBaseline: TextBaseline.alphabetic,
                                    children: <Widget>[
                                      Expanded(
                                        child: Text(
                                          session.name,
                                          maxLines: 1,
                                          overflow: TextOverflow.ellipsis,
                                          style: kit.heading,
                                        ),
                                      ),
                                      const SizedBox(width: Space.sm),
                                      Text(
                                        time,
                                        maxLines: 1,
                                        softWrap: false,
                                        style: kit.meta.copyWith(
                                          color: kit.textFaint,
                                        ),
                                      ),
                                    ],
                                  ),
                                ),
                                const SizedBox(height: 1),
                                Row(
                                  children: <Widget>[
                                    if (session.control ==
                                        SessionControl.viewOnly)
                                      Padding(
                                        padding: const EdgeInsets.only(
                                          right: 4,
                                        ),
                                        child: Icon(
                                          Icons.visibility_outlined,
                                          size: 13,
                                          color: kit.textMuted,
                                        ),
                                      ),
                                    Expanded(
                                      child: ExcludeSemantics(
                                        child: Text.rich(
                                          TextSpan(
                                            children: <InlineSpan>[
                                              TextSpan(
                                                text: session.project,
                                                style: kit.meta,
                                              ),
                                              if (activity != null)
                                                TextSpan(
                                                  text: '  ·  ',
                                                  style: kit.meta.copyWith(
                                                    color: kit.textFaint,
                                                  ),
                                                ),
                                              if (activity != null)
                                                TextSpan(
                                                  text: activity,
                                                  style: kit.meta.copyWith(
                                                    color: needs
                                                        ? kit.accentText
                                                        : kit.text,
                                                  ),
                                                ),
                                            ],
                                          ),
                                          maxLines: 1,
                                          overflow: TextOverflow.ellipsis,
                                        ),
                                      ),
                                    ),
                                    if (hasSubagents) ...<Widget>[
                                      const SizedBox(width: Space.sm),
                                      Semantics(
                                        button: true,
                                        label: expanded
                                            ? l10n.sessionsSubagentsHide(
                                                session.subagents.length,
                                              )
                                            : l10n.sessionsSubagentsShow(
                                                session.subagents.length,
                                              ),
                                        excludeSemantics: true,
                                        onTap: onToggleExpand,
                                        child: GestureDetector(
                                          behavior: HitTestBehavior.opaque,
                                          onTap: onToggleExpand,
                                          child: Padding(
                                            padding: const EdgeInsets.symmetric(
                                              vertical: Space.sm,
                                              horizontal: 2,
                                            ),
                                            child: Row(
                                              mainAxisSize: MainAxisSize.min,
                                              children: <Widget>[
                                                Icon(
                                                  Icons.account_tree_outlined,
                                                  size: 12,
                                                  color: running > 0
                                                      ? kit.working
                                                      : kit.textMuted,
                                                ),
                                                const SizedBox(width: 3),
                                                Text(
                                                  '${session.subagents.length}',
                                                  style: kit.meta.copyWith(
                                                    color: running > 0
                                                        ? kit.working
                                                        : kit.textMuted,
                                                  ),
                                                ),
                                                AnimatedRotation(
                                                  turns: expanded ? 0.5 : 0,
                                                  duration: motionDuration(
                                                    context,
                                                  ),
                                                  child: Icon(
                                                    Icons.expand_more,
                                                    size: 16,
                                                    color: kit.textMuted,
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
                              ],
                            ),
                          ),
                        ],
                      ),
                    ),
                  ),
                ),
              ),
            ),
          ),
          ?trailing,
        ],
      ),
    );
  }
}

class SubagentTree extends StatelessWidget {
  const SubagentTree({super.key, required this.agents});
  final List<Subagent> agents;

  @override
  Widget build(BuildContext context) {
    return Column(
      children: <Widget>[
        for (int i = 0; i < agents.length; i++)
          SubagentRow(agent: agents[i], last: i == agents.length - 1),
      ],
    );
  }
}

class SubagentRow extends StatelessWidget {
  const SubagentRow({super.key, required this.agent, this.last = true});
  final Subagent agent;
  final bool last;

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final Kit kit = Kit.of(context);
    final SessionState state = agent.running
        ? SessionState.running
        : SessionState.done;
    return Semantics(
      label:
          '${agent.type}, ${agent.description}, ${agent.model}, ${state.label(l10n)}',
      excludeSemantics: true,
      child: ConstrainedBox(
        constraints: const BoxConstraints(minHeight: Metric.subagentRowHeight),
        child: IntrinsicHeight(
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: <Widget>[
              const SizedBox(width: Space.sm + Space.md + 20 - 8),
              SizedBox(
                width: 28,
                child: CustomPaint(
                  painter: _ElbowPainter(color: kit.hairline, last: last),
                ),
              ),
              Expanded(
                child: Padding(
                  padding: const EdgeInsets.fromLTRB(
                    0,
                    Space.sm,
                    Space.lg,
                    Space.sm,
                  ),
                  child: Row(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: <Widget>[
                      Padding(
                        padding: const EdgeInsets.only(top: 5),
                        child: Icon(
                          state.icon,
                          size: 14,
                          color: state.color(kit),
                        ),
                      ),
                      const SizedBox(width: Space.sm),
                      Expanded(
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: <Widget>[
                            Text.rich(
                              TextSpan(
                                children: <InlineSpan>[
                                  TextSpan(text: agent.type, style: kit.label),
                                  TextSpan(
                                    text: '  ${agent.description}',
                                    style: kit.meta,
                                  ),
                                ],
                              ),
                            ),
                            if (agent.model.isNotEmpty)
                              Text(
                                agent.model,
                                style: kit.mono.copyWith(
                                  fontSize: 12,
                                  color: kit.textFaint,
                                ),
                              ),
                          ],
                        ),
                      ),
                      const SizedBox(width: Space.sm),
                      Text(
                        state.label(l10n),
                        style: kit.label.copyWith(
                          color: state.labelColor(kit),
                          fontSize: 12.5,
                        ),
                      ),
                    ],
                  ),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _ElbowPainter extends CustomPainter {
  const _ElbowPainter({required this.color, required this.last});
  final Color color;
  final bool last;

  @override
  void paint(Canvas canvas, Size size) {
    final Paint paint = Paint()
      ..color = color
      ..strokeWidth = 1.5
      ..style = PaintingStyle.stroke;
    const double x = 2;
    final double mid = size.height < 26 ? size.height / 2 : 18;
    canvas.drawLine(
      const Offset(x, 0),
      Offset(x, last ? mid : size.height),
      paint,
    );
    canvas.drawLine(Offset(x, mid), Offset(x + 18, mid), paint);
  }

  @override
  bool shouldRepaint(_ElbowPainter oldDelegate) =>
      oldDelegate.color != color || oldDelegate.last != last;
}
