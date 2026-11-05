import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/nav_state.dart';
import '../../app/theme/tokens.dart';
import '../../app/widgets/common.dart';
import '../../app/widgets/motion.dart';
import '../../core/protocol/models.dart';
import '../../l10n/app_localizations.dart';
import 'archive_store.dart';
import 'new_session_sheet.dart';
import '../chat/takeover_flow.dart';
import 'session_row.dart';
import 'takeover_row_button.dart';
import 'sessions_notifier.dart';
import 'status_style.dart';

/// Which sessions have their subagent rows open. Remembered while the app runs.
final NotifierProvider<ExpandedSessionsNotifier, Set<String>>
expandedSessionsProvider =
    NotifierProvider<ExpandedSessionsNotifier, Set<String>>(
      ExpandedSessionsNotifier.new,
    );

class ExpandedSessionsNotifier extends Notifier<Set<String>> {
  @override
  Set<String> build() => <String>{};

  void toggle(String id) => state = state.contains(id)
      ? (<String>{...state}..remove(id))
      : <String>{...state, id};
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
  String? _project;

  void _select(Session session) {
    ref.read(selectedSessionIdProvider.notifier).select(session.id);
    widget.onOpenChat?.call();
  }

  @override
  Widget build(BuildContext context) {
    final SessionsState sessions = ref.watch(sessionsProvider);
    final Set<String> archived = hiddenSessionIds(
      ref.watch(archivedSessionsProvider),
      sessions.sessions,
    );
    return SessionsView(
      state: sessions,
      expanded: ref.watch(expandedSessionsProvider),
      selectedId: ref.watch(selectedSessionIdProvider),
      archived: archived,
      query: _query,
      project: _project,
      onQuery: (String value) => setState(() => _query = value),
      onProject: (String? value) => setState(() => _project = value),
      onSelect: _select,
      onToggleExpand: (String id) =>
          ref.read(expandedSessionsProvider.notifier).toggle(id),
      onRefresh: () => ref.read(sessionsProvider.notifier).refresh(),
      onNewSession: () => unawaited(showNewSessionSheet(context)),
      onArchive: (Session session) {
        final AppL10n l10n = AppL10n.of(context);
        final ScaffoldMessengerState messenger = ScaffoldMessenger.of(context);
        ref.read(archivedSessionsProvider.notifier).archive(session.id);
        messenger
          ..hideCurrentSnackBar()
          ..showSnackBar(
            SnackBar(
              content: Text(l10n.archivedToast(session.name)),
              action: SnackBarAction(
                label: l10n.undo,
                onPressed: () => ref
                    .read(archivedSessionsProvider.notifier)
                    .restore(session.id),
              ),
            ),
          );
      },
      onTakeover: (Session session) =>
          unawaited(runTakeover(context, ref, session)),
    );
  }
}

/// The list without any provider: loading, error, empty, no match, or the groups. Previews and tests drive it directly.
class SessionsView extends StatelessWidget {
  const SessionsView({
    super.key,
    required this.state,
    required this.expanded,
    required this.selectedId,
    required this.query,
    required this.project,
    required this.onQuery,
    required this.onProject,
    required this.onSelect,
    required this.onToggleExpand,
    required this.onRefresh,
    required this.onNewSession,
    required this.onArchive,
    required this.onTakeover,
    this.archived = const <String>{},
    this.now,
  });

  final SessionsState state;
  final Set<String> expanded;
  final String? selectedId;
  final Set<String> archived;
  final String query;
  final String? project;
  final void Function(String) onQuery;
  final void Function(String?) onProject;
  final void Function(Session) onSelect;
  final void Function(String) onToggleExpand;
  final Future<void> Function() onRefresh;
  final VoidCallback onNewSession;
  final void Function(Session) onArchive;
  final void Function(Session) onTakeover;
  final DateTime? now;

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final Kit kit = Kit.of(context);
    final DateTime clock = now ?? DateTime.now();
    final List<Session> visible = state.sessions
        .where(
          (Session s) =>
              !archived.contains(s.id) &&
              (project == null || s.project == project),
        )
        .toList();
    final Map<SessionState, List<Session>> groups = groupSessions(
      visible,
      query: query,
    );
    final List<String> projects = <String>{
      for (final Session s in state.sessions) s.project,
    }.toList()..sort();

    final Widget header = Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        Padding(
          padding: const EdgeInsets.fromLTRB(
            Space.lg,
            Space.sm,
            Space.lg,
            Space.sm,
          ),
          child: Row(
            children: <Widget>[
              Expanded(
                child: TextField(
                  onChanged: onQuery,
                  style: kit.body,
                  decoration: InputDecoration(
                    hintText: l10n.searchHint,
                    prefixIcon: Icon(
                      Icons.search,
                      size: 20,
                      color: kit.textMuted,
                    ),
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
                  onPressed: onNewSession,
                  icon: Icon(Icons.add, color: kit.onAccent),
                  tooltip: l10n.newSession,
                  constraints: const BoxConstraints(
                    minWidth: Metric.minTarget,
                    minHeight: Metric.minTarget,
                  ),
                  style: IconButton.styleFrom(
                    backgroundColor: kit.accent,
                    shape: RoundedRectangleBorder(
                      borderRadius: BorderRadius.circular(Radii.md),
                    ),
                  ),
                ),
              ),
            ],
          ),
        ),
        if (projects.length > 1)
          SizedBox(
            height: 40,
            child: ListView(
              scrollDirection: Axis.horizontal,
              padding: const EdgeInsets.symmetric(horizontal: Space.lg),
              children: <Widget>[
                _ProjectChip(
                  label: l10n.allProjects,
                  selected: project == null,
                  onTap: () => onProject(null),
                ),
                for (final String name in projects)
                  _ProjectChip(
                    label: name,
                    selected: project == name,
                    onTap: () => onProject(project == name ? null : name),
                  ),
              ],
            ),
          ),
        const SizedBox(height: Space.xs),
      ],
    );

    // One scrolling list for every state: the search field and chips are its first item, so they scroll away with
    // the rows (nothing slides under a pinned bar) and the field keeps its focus while the list below it changes.
    final double stateHeight = math.max(
      320,
      MediaQuery.sizeOf(context).height * 0.5,
    );
    Widget fill(Widget child) => SizedBox(height: stateHeight, child: child);
    int rowIndex = 0;
    final List<Widget> content;
    if (state.phase == LoadPhase.loading && state.sessions.isEmpty) {
      content = <Widget>[fill(const SkeletonRows())];
    } else if (state.phase == LoadPhase.error && state.sessions.isEmpty) {
      content = <Widget>[
        fill(
          ErrorState(
            message: l10n.loadFailed,
            onRetry: () => unawaited(onRefresh()),
          ),
        ),
      ];
    } else if (state.sessions.isEmpty) {
      content = <Widget>[
        fill(
          StateMessage(
            pet: 'sit-1',
            title: l10n.sessionsEmptyTitle,
            body: l10n.sessionsEmptyBody,
            action: SecondaryButton(
              label: l10n.newSession,
              icon: Icons.add,
              onPressed: onNewSession,
            ),
          ),
        ),
      ];
    } else if (groups.isEmpty) {
      content = <Widget>[
        fill(
          StateMessage(
            pet: 'look-2',
            title: query.isNotEmpty
                ? l10n.searchNoMatch(query)
                : l10n.sessionsFilteredEmpty,
          ),
        ),
      ];
    } else {
      content = <Widget>[
        for (final MapEntry<SessionState, List<Session>> group
            in groups.entries) ...<Widget>[
          _GroupHeader(state: group.key, count: group.value.length),
          for (final Session session in group.value) ...<Widget>[
            FlipOnMove(
              key: ValueKey<String>('row-${session.id}'),
              child: Entrance(
                // The first screenful cascades in, 40 ms apart; later rows (scrolled to, searched) come at once.
                delay: Duration(milliseconds: 40 * math.min(rowIndex++, 9)),
                rise: 12,
                child: _SwipeRow(
                  session: session,
                  onArchive: () => onArchive(session),
                  onTakeover: canTakeOver(session)
                      ? () => onTakeover(session)
                      : null,
                  child: SessionRow(
                    session: session,
                    now: clock,
                    selected: session.id == selectedId,
                    expanded: expanded.contains(session.id),
                    onTap: () => onSelect(session),
                    onToggleExpand: session.subagents.isEmpty
                        ? null
                        : () => onToggleExpand(session.id),
                    onLongPress: () => unawaited(_showMenu(context, session)),
                  ),
                ),
              ),
            ),
            ExpandReveal(
              expanded: expanded.contains(session.id),
              child: SubagentTree(agents: session.subagents),
            ),
          ],
        ],
        const SizedBox(height: Space.xl),
      ];
    }

    return RefreshIndicator(
      color: kit.accent,
      backgroundColor: kit.card,
      onRefresh: onRefresh,
      child: ListView(
        physics: const AlwaysScrollableScrollPhysics(),
        children: <Widget>[header, ...content],
      ),
    );
  }

  /// Long-press: what the swipes do, as a menu (swipes are not discoverable).
  Future<void> _showMenu(BuildContext context, Session session) {
    final AppL10n l10n = AppL10n.of(context);
    final Kit kit = Kit.of(context);
    return showModalBottomSheet<void>(
      context: context,
      showDragHandle: true,
      builder: (BuildContext sheet) => SafeArea(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: <Widget>[
            Padding(
              padding: const EdgeInsets.fromLTRB(
                Space.lg,
                0,
                Space.lg,
                Space.sm,
              ),
              child: Align(
                alignment: Alignment.centerLeft,
                child: Text(
                  session.name,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: kit.heading,
                ),
              ),
            ),
            if (canTakeOver(session))
              ListTile(
                leading: Icon(Icons.login, color: kit.textMuted),
                title: Text(l10n.takeover, style: kit.body),
                minVerticalPadding: Space.md,
                onTap: () {
                  Navigator.of(sheet).pop();
                  onTakeover(session);
                },
              ),
            ListTile(
              leading: Icon(Icons.archive_outlined, color: kit.textMuted),
              title: Text(l10n.archive, style: kit.body),
              minVerticalPadding: Space.md,
              onTap: () {
                Navigator.of(sheet).pop();
                onArchive(session);
              },
            ),
          ],
        ),
      ),
    );
  }
}

class _ProjectChip extends StatelessWidget {
  const _ProjectChip({
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

/// Swipe right: take a view-only session over. Swipe left: archive. Both also exist inside the session (the
/// Tiếp quản button, the chat), since swipes are not discoverable.
class _SwipeRow extends StatelessWidget {
  const _SwipeRow({
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
        label: l10n.takeoverShort,
        alignment: Alignment.centerLeft,
      ),
      secondaryBackground: _SwipeBackground(
        color: kit.raised,
        foreground: kit.text,
        icon: Icons.archive_outlined,
        label: l10n.archive,
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

class _GroupHeader extends StatelessWidget {
  const _GroupHeader({required this.state, required this.count});
  final SessionState state;
  final int count;

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final Kit kit = Kit.of(context);
    return Semantics(
      header: true,
      label: '${state.label(l10n)}, $count session',
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

/// Tapping a notification or an `open` event: show the Session tab.
void showSessionsTab(WidgetRef ref) =>
    ref.read(activeTabProvider.notifier).show(0);
