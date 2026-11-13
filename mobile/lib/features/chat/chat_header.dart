import 'package:flutter/material.dart';

import '../../app/theme/tokens.dart';
import '../../app/widgets/kit.dart';
import '../../core/protocol/models.dart';
import '../../l10n/app_localizations.dart';
import '../sessions/status_style.dart';

/// "148.8k" / "1.2M".
String compactTokens(int tokens) {
  if (tokens >= 1000000) return '${(tokens / 1000000).toStringAsFixed(1)}M';
  if (tokens >= 1000) return '${(tokens / 1000).toStringAsFixed(1)}k';
  return '$tokens';
}

/// The Mac's chat header on a phone, in two lines that never grow past them: the avatar with its status badge, the
/// name, the project and control, and the bell and Macs buttons; then ONE line of facts (status, context percentage
/// with a small bar, the account's 5h and week limits). The facts line scrolls sideways when it does not fit; it never
/// wraps, so no single item is left alone on a second line. The full numbers are in each fact's tooltip (tap or hold).
class ChatHeaderView extends StatelessWidget {
  const ChatHeaderView({super.key, required this.session, required this.muted, required this.onToggleMute, this.onOpenMacs, this.onBack});
  final Session session;
  final bool muted;
  final VoidCallback onToggleMute;

  /// The laptop button (paired Macs); the generic app bar is gone on this tab, so its action lives here.
  final VoidCallback? onOpenMacs;

  /// Shown when the chat is a page pushed over the session list (a phone); the wide layout has the list beside it.
  final VoidCallback? onBack;

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final Kit kit = Kit.of(context);
    final String control = switch (session.control) {
      SessionControl.console => l10n.controlConsole,
      SessionControl.job => l10n.controlJob,
      SessionControl.viewOnly => l10n.viewOnly,
    };
    final bool large = MediaQuery.textScalerOf(context).scale(10) > 11.5;
    final SessionFacts? facts = session.facts;
    final bool hasContext = facts?.contextTokens != null && facts!.contextTokens! > 0;
    final List<LimitMeter> limits = facts?.limits ?? const <LimitMeter>[];
    final List<Widget> chips = <Widget>[
      StatePill(state: session.state),
      if (hasContext) _contextFact(l10n, facts, short: large),
      for (final LimitMeter limit in limits) _limitFact(l10n, limit),
    ];
    return Semantics(
      container: true,
      label: '${session.name}, ${session.project}, ${session.state.label(l10n)}, $control',
      child: Container(
        padding: EdgeInsets.fromLTRB(onBack == null ? Space.lg : Space.xs, Space.sm, Space.xs, Space.sm),
        decoration: BoxDecoration(color: kit.window, border: Border(bottom: BorderSide(color: kit.hairline))),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: <Widget>[
            Row(
              crossAxisAlignment: CrossAxisAlignment.center,
              children: <Widget>[
                if (onBack != null)
                  IconButton(
                    onPressed: onBack,
                    tooltip: MaterialLocalizations.of(context).backButtonTooltip,
                    icon: const BackButtonIcon(),
                    color: kit.textMuted,
                  ),
                KitAvatar(name: session.name, size: 36, badge: StatusDot(state: session.state, size: 12, ring: true)),
                const SizedBox(width: Space.md),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: <Widget>[
                      Text(session.name, maxLines: 2, overflow: TextOverflow.ellipsis, style: kit.title.copyWith(fontSize: 16.5)),
                      const SizedBox(height: 1),
                      Row(
                        children: <Widget>[
                          Flexible(child: Text(session.project, maxLines: 1, overflow: TextOverflow.ellipsis, style: kit.meta)),
                          const SizedBox(width: Space.sm),
                          Icon(session.control == SessionControl.viewOnly ? Icons.visibility_outlined : Icons.terminal, size: 13, color: kit.textMuted),
                          const SizedBox(width: 3),
                          Text(control, maxLines: 1, softWrap: false, style: kit.meta),
                        ],
                      ),
                    ],
                  ),
                ),
                IconButton(
                  tooltip: muted ? l10n.unmuteSession : l10n.muteSession,
                  constraints: const BoxConstraints(minWidth: 44, minHeight: Metric.minTarget),
                  padding: EdgeInsets.zero,
                  icon: Icon(muted ? Icons.notifications_off_outlined : Icons.notifications_none, size: 22, color: muted ? kit.accentText : kit.textMuted),
                  onPressed: onToggleMute,
                ),
                if (onOpenMacs != null)
                  IconButton(
                    tooltip: l10n.openMacs,
                    constraints: const BoxConstraints(minWidth: 44, minHeight: Metric.minTarget),
                    padding: EdgeInsets.zero,
                    icon: Icon(Icons.laptop_mac, size: 22, color: kit.textMuted),
                    onPressed: onOpenMacs,
                  ),
              ],
            ),
            const SizedBox(height: Space.sm),
            SingleChildScrollView(
              scrollDirection: Axis.horizontal,
              padding: const EdgeInsets.only(right: Space.md),
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.center,
                children: <Widget>[
                  for (int i = 0; i < chips.length; i++) ...<Widget>[if (i > 0) const SizedBox(width: Space.md), chips[i]],
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }

  /// "Ngữ cảnh 58%" with a bar; the tokens over the window are in the tooltip. Without a known window the tokens show.
  Widget _contextFact(AppL10n l10n, SessionFacts facts, {required bool short}) {
    final double? fraction = facts.contextFraction;
    final String full = facts.contextWindow != null
        ? '${compactTokens(facts.contextTokens!)} / ${compactTokens(facts.contextWindow!)}'
        : compactTokens(facts.contextTokens!);
    return Tooltip(
      message: '${l10n.chatContext}: $full',
      triggerMode: TooltipTriggerMode.tap,
      child: KitMeter(
        label: short ? '' : l10n.chatContext,
        icon: short ? Icons.data_usage : null,
        value: fraction != null ? '${(fraction * 100).round()}%' : compactTokens(facts.contextTokens!),
        fraction: fraction,
        width: 72,
      ),
    );
  }

  Widget _limitFact(AppL10n l10n, LimitMeter limit) {
    final String label = switch (limit.label.toLowerCase()) {
      '5h' => '5h',
      'week' => l10n.limitWeek,
      _ => limit.label,
    };
    return Tooltip(
      message: '$label: ${limit.percent.round()}%',
      triggerMode: TooltipTriggerMode.tap,
      child: KitMeter(label: label, value: '${limit.percent.round()}%', fraction: limit.percent / 100, width: 48),
    );
  }
}
