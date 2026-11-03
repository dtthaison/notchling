import 'package:flutter/material.dart';

import '../../app/theme/tokens.dart';
import '../../core/protocol/models.dart';
import '../../l10n/app_localizations.dart';
import 'status_style.dart';

/// One main session: status dot + icon + words, name, project, running-subagent badge, time.
/// The disclosure arrow opens the subagent rows. At least 48 dp tall; grows with large text.
class SessionRow extends StatelessWidget {
  const SessionRow({
    super.key,
    required this.session,
    required this.now,
    required this.onTap,
    this.selected = false,
    this.expanded = false,
    this.onToggleExpand,
    this.trailing,
  });

  final Session session;
  final DateTime now;
  final bool selected;
  final bool expanded;
  final VoidCallback onTap;
  final VoidCallback? onToggleExpand;

  /// An action at the end of the row ("Tiếp quản" on a view-only session), outside the row's own tap area.
  final Widget? trailing;

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final int running = session.runningSubagents;
    final bool hasSubagents = session.subagents.isNotEmpty;
    final String time = relativeTime(l10n, session.lastActivity, now);
    final String spoken = <String>[
      session.name,
      session.project,
      session.state.label(l10n),
      if (running > 0) l10n.runningSubagents(running),
      if (session.control == SessionControl.viewOnly) l10n.viewOnly,
      time,
    ].join(', ');

    return Material(
      color: selected ? Token.surfaceRaised : Colors.transparent,
      child: Stack(
        children: <Widget>[
          if (selected) Positioned(left: 0, top: 0, bottom: 0, child: Container(width: 2, color: Token.primary)),
          Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: <Widget>[
              if (hasSubagents)
                Semantics(
                  button: true,
                  label: expanded ? l10n.subagentsHide(session.subagents.length) : l10n.subagentsShow(session.subagents.length),
                  excludeSemantics: true,
                  onTap: onToggleExpand,
                  child: InkResponse(
                    onTap: onToggleExpand,
                    radius: 24,
                    child: SizedBox(
                      width: Metric.minTarget,
                      height: Metric.rowHeight,
                      child: AnimatedRotation(
                        turns: expanded ? 0.25 : 0,
                        duration: MediaQuery.disableAnimationsOf(context) ? Duration.zero : const Duration(milliseconds: 150),
                        child: const Icon(Icons.chevron_right, size: 20, color: Token.muted),
                      ),
                    ),
                  ),
                )
              else
                const SizedBox(width: Space.sm),
              Expanded(
                child: Semantics(
                  button: true,
                  selected: selected,
                  label: spoken,
                  excludeSemantics: true,
                  onTap: onTap,
                  child: InkWell(
                    onTap: onTap,
                    child: ConstrainedBox(
                      constraints: const BoxConstraints(minHeight: Metric.rowHeight),
                      child: Padding(
                        padding: const EdgeInsets.fromLTRB(Space.sm, Space.sm, Space.lg, Space.sm),
                        child: Row(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: <Widget>[
                            Padding(
                              padding: const EdgeInsets.only(top: 6),
                              child: Row(mainAxisSize: MainAxisSize.min, children: <Widget>[
                                StatusDot(state: session.state),
                                const SizedBox(width: 6),
                                Icon(session.state.icon, size: 12, color: session.state.labelColor),
                              ]),
                            ),
                            const SizedBox(width: Space.md),
                            Expanded(
                              child: Column(
                                crossAxisAlignment: CrossAxisAlignment.start,
                                children: <Widget>[
                                  Text(session.name, maxLines: 2, overflow: TextOverflow.ellipsis, style: TypeToken.bodyMd),
                                  Wrap(
                                    spacing: Space.sm,
                                    crossAxisAlignment: WrapCrossAlignment.center,
                                    children: <Widget>[
                                      Text(session.project, style: TypeToken.bodySm),
                                      if (running > 0)
                                        Row(mainAxisSize: MainAxisSize.min, children: <Widget>[
                                          const Icon(Icons.account_tree_outlined, size: 11, color: Token.statusRunning),
                                          const SizedBox(width: 2),
                                          Text('$running', style: TypeToken.labelSm.copyWith(color: Token.statusRunning)),
                                        ]),
                                      if (session.control == SessionControl.viewOnly)
                                        const Icon(Icons.visibility_outlined, size: 12, color: Token.muted),
                                      Text(time, style: TypeToken.labelSm),
                                    ],
                                  ),
                                ],
                              ),
                            ),
                            if (session.state == SessionState.needsInput) ...<Widget>[const SizedBox(width: Space.sm), const InputBadge()],
                          ],
                        ),
                      ),
                    ),
                  ),
                ),
              ),
              if (trailing != null) Padding(padding: const EdgeInsets.only(right: Space.sm, top: 2), child: trailing),
            ],
          ),
        ],
      ),
    );
  }
}

/// A subagent of a session: type · description · model (mono) · state. View-only, 44 dp tall at least.
class SubagentRow extends StatelessWidget {
  const SubagentRow({super.key, required this.agent});
  final Subagent agent;

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final SessionState state = agent.running ? SessionState.running : SessionState.done;
    return Semantics(
      label: '${agent.type}, ${agent.description}, ${agent.model}, ${state.label(l10n)}',
      excludeSemantics: true,
      child: ConstrainedBox(
        constraints: const BoxConstraints(minHeight: Metric.subagentRowHeight),
        child: Padding(
          padding: const EdgeInsets.fromLTRB(Space.xl * 2, Space.sm, Space.lg, Space.sm),
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: <Widget>[
              Padding(padding: const EdgeInsets.only(top: 5), child: StatusDot(state: state)),
              const SizedBox(width: Space.md),
              Expanded(
                child: Wrap(
                  spacing: Space.sm,
                  children: <Widget>[
                    Text(agent.type, style: TypeToken.bodySm),
                    Text(agent.description, style: TypeToken.bodySm),
                    if (agent.model.isNotEmpty) Text(agent.model, style: TypeToken.codeSm),
                  ],
                ),
              ),
              const SizedBox(width: Space.sm),
              Text(state.label(l10n), style: TypeToken.labelSm.copyWith(color: state.labelColor)),
            ],
          ),
        ),
      ),
    );
  }
}
