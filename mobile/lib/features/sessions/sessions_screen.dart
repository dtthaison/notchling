import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/nav_state.dart';
import '../../app/theme/tokens.dart';
import '../../app/widgets/common.dart';
import '../../core/protocol/models.dart';
import '../../l10n/app_localizations.dart';
import 'new_session_sheet.dart';
import '../chat/takeover_flow.dart';
import 'session_row.dart';
import 'takeover_row_button.dart';
import 'sessions_notifier.dart';
import 'status_style.dart';

/// Which sessions have their subagent rows open. Remembered while the app runs.
final NotifierProvider<ExpandedSessionsNotifier, Set<String>> expandedSessionsProvider =
    NotifierProvider<ExpandedSessionsNotifier, Set<String>>(ExpandedSessionsNotifier.new);

class ExpandedSessionsNotifier extends Notifier<Set<String>> {
  @override
  Set<String> build() => <String>{};

  void toggle(String id) => state = state.contains(id) ? (<String>{...state}..remove(id)) : <String>{...state, id};
}

class SessionsScreen extends ConsumerStatefulWidget {
  const SessionsScreen({super.key, this.onOpenChat});

  /// Called after a session is picked; on a phone it switches to the Chat tab.
  final VoidCallback? onOpenChat;

  @override
  ConsumerState<SessionsScreen> createState() => _SessionsScreenState();
}

class _SessionsScreenState extends ConsumerState<SessionsScreen> {
  String _query = '';

  void _select(Session session) {
    ref.read(selectedSessionIdProvider.notifier).select(session.id);
    widget.onOpenChat?.call();
  }

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final SessionsState sessions = ref.watch(sessionsProvider);
    final Set<String> expanded = ref.watch(expandedSessionsProvider);
    final String? selectedId = ref.watch(selectedSessionIdProvider);
    final DateTime now = DateTime.now();
    final Map<SessionState, List<Session>> groups = groupSessions(sessions.sessions, query: _query);

    Widget body;
    if (sessions.phase == LoadPhase.loading && sessions.sessions.isEmpty) {
      body = const SkeletonRows();
    } else if (sessions.phase == LoadPhase.error && sessions.sessions.isEmpty) {
      body = ErrorState(message: l10n.loadFailed, onRetry: () => unawaited(ref.read(sessionsProvider.notifier).refresh()));
    } else if (sessions.sessions.isEmpty) {
      body = StateMessage(
        icon: Icons.terminal,
        title: l10n.sessionsEmptyTitle,
        body: l10n.sessionsEmptyBody,
        action: SecondaryButton(label: l10n.newSession, icon: Icons.add, onPressed: () => unawaited(showNewSessionSheet(context))),
      );
    } else if (groups.isEmpty) {
      body = StateMessage(icon: Icons.search_off, title: l10n.searchNoMatch(_query));
    } else {
      body = RefreshIndicator(
        color: Token.primary,
        backgroundColor: Token.surfaceRaised,
        onRefresh: () => ref.read(sessionsProvider.notifier).refresh(),
        child: ListView(
          physics: const AlwaysScrollableScrollPhysics(),
          children: <Widget>[
            for (final MapEntry<SessionState, List<Session>> group in groups.entries) ...<Widget>[
              _GroupHeader(state: group.key, count: group.value.length),
              for (final Session session in group.value) ...<Widget>[
                SessionRow(
                  session: session,
                  now: now,
                  selected: session.id == selectedId,
                  expanded: expanded.contains(session.id),
                  onTap: () => _select(session),
                  onToggleExpand: session.subagents.isEmpty ? null : () => ref.read(expandedSessionsProvider.notifier).toggle(session.id),
                  trailing: canTakeOver(session) ? TakeoverRowButton(session: session) : null,
                ),
                if (expanded.contains(session.id)) for (final Subagent agent in session.subagents) SubagentRow(agent: agent),
              ],
            ],
            const SizedBox(height: Space.xl),
          ],
        ),
      );
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        Padding(
          padding: const EdgeInsets.fromLTRB(Space.lg, Space.md, Space.lg, Space.sm),
          child: Row(
            children: <Widget>[
              Expanded(
                child: TextField(
                  onChanged: (String value) => setState(() => _query = value),
                  style: TypeToken.bodyMd,
                  decoration: InputDecoration(
                    hintText: l10n.searchHint,
                    prefixIcon: const Icon(Icons.search, size: 18, color: Token.muted),
                    isDense: false,
                  ),
                ),
              ),
              const SizedBox(width: Space.sm),
              Semantics(
                button: true,
                label: l10n.newSession,
                excludeSemantics: true,
                child: IconButton(
                  onPressed: () => unawaited(showNewSessionSheet(context)),
                  icon: const Icon(Icons.add, color: Token.onSurface),
                  tooltip: l10n.newSession,
                  constraints: const BoxConstraints(minWidth: Metric.minTarget, minHeight: Metric.minTarget),
                  style: IconButton.styleFrom(
                      backgroundColor: Token.surfaceRaised, shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(Radii.md), side: const BorderSide(color: Token.border))),
                ),
              ),
            ],
          ),
        ),
        Expanded(child: body),
      ],
    );
  }
}

class _GroupHeader extends StatelessWidget {
  const _GroupHeader({required this.state, required this.count});
  final SessionState state;
  final int count;

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    return Semantics(
      header: true,
      label: '${state.label(l10n)}, $count session',
      excludeSemantics: true,
      child: Container(
        padding: const EdgeInsets.fromLTRB(Space.lg, Space.md, Space.lg, Space.xs),
        decoration: const BoxDecoration(border: Border(top: BorderSide(color: Token.border))),
        child: Row(children: <Widget>[
          Text(state.label(l10n), style: TypeToken.labelSm),
          const SizedBox(width: Space.sm),
          AnimatedSwitcher(
            duration: reduceMotion(context) ? Duration.zero : const Duration(milliseconds: 200),
            transitionBuilder: (Widget child, Animation<double> animation) => FadeTransition(opacity: animation, child: child),
            child: Text('$count', key: ValueKey<int>(count), style: TypeToken.labelSm),
          ),
        ]),
      ),
    );
  }
}

/// Tapping a notification or an `open` event: show the Session tab.
void showSessionsTab(WidgetRef ref) => ref.read(activeTabProvider.notifier).show(0);
