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
import '../macs/computer_heading.dart';
import '../macs/computers.dart';
import '../macs/macs_notifier.dart';
import '../macs/session_ref.dart';
import 'archive_store.dart';
import 'session_list_parts.dart';
import 'new_session_sheet.dart';
import '../chat/takeover_flow.dart';
import 'session_row.dart';
import 'takeover_row_button.dart';
import 'sessions_notifier.dart';

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

  final VoidCallback? onOpenChat;

  @override
  ConsumerState<SessionsScreen> createState() => _SessionsScreenState();
}

class _SessionsScreenState extends ConsumerState<SessionsScreen> {
  String _query = '';
  String? _project;

  @override
  Widget build(BuildContext context) {
    final MacsState macs = ref.watch(macsProvider);
    final String active = macs.active?.id ?? '';
    final bool perComputer = macs.macs.length > 1;
    final List<ComputerSessions> all = perComputer ? ref.watch(allComputersSessionsProvider) : const <ComputerSessions>[];
    final Map<String, int> archive = ref.watch(archivedSessionsProvider);
    SessionRef refOf(String id) => perComputer ? (SessionRef.parse(id) ?? SessionRef(active, id)) : SessionRef(active, id);
    String keyOf(Session session) {
      final SessionRef at = refOf(session.id);
      return storageKey(macs.scopeOf(at.host), at.id);
    }

    final SessionsState sessions = perComputer ? mergeComputerSessions(all) : ref.watch(sessionsProvider);
    final Set<String> archived = perComputer
        ? <String>{
            for (final ComputerSessions c in all)
              for (final String id in hiddenSessionIdsIn(archive, ref.watch(computerSessionsProvider(c.computer.id)).sessions, c.scope))
                SessionRef(c.computer.id, id).qualified,
          }
        : hiddenSessionIdsIn(archive, sessions.sessions, macs.scopeOf(active));
    final Set<String> expanded = <String>{
      for (final String key in ref.watch(expandedSessionsProvider))
        if (perComputer || active.isEmpty)
          key
        else if (SessionRef.parse(key) case final SessionRef r when r.host == active)
          r.id,
    };
    return SessionsView(
      state: sessions,
      computers: <ComputerHeading>[
        for (final ComputerSessions c in all) ComputerHeading(id: c.computer.id, name: c.computer.name, status: c.status),
      ],
      expanded: expanded,
      selectedId: perComputer ? ref.watch(selectedSessionProvider)?.qualified : ref.watch(selectedSessionIdProvider),
      archived: archived,
      query: _query,
      project: _project,
      onQuery: (String value) => setState(() => _query = value),
      onProject: (String? value) => setState(() => _project = value),
      onSelect: (Session session) {
        ref.read(selectedSessionProvider.notifier).select(refOf(session.id));
        widget.onOpenChat?.call();
      },
      onToggleExpand: (String id) => ref.read(expandedSessionsProvider.notifier).toggle(refOf(id).qualified),
      onRefresh: () => perComputer
          ? Future.wait(<Future<void>>[for (final ComputerSessions c in all) ref.read(computerSessionsProvider(c.computer.id).notifier).refresh()])
          : ref.read(sessionsProvider.notifier).refresh(),
      onNewSession: () => unawaited(showNewSessionSheet(context)),
      onArchive: (Session session) {
        final AppL10n l10n = AppL10n.of(context);
        final ScaffoldMessengerState messenger = ScaffoldMessenger.of(context);
        final String key = keyOf(session);
        ref.read(archivedSessionsProvider.notifier).archive(key);
        messenger
          ..hideCurrentSnackBar()
          ..showSnackBar(
            SnackBar(
              content: Text(l10n.archivedToast(session.name)),
              action: SnackBarAction(
                label: l10n.undo,
                onPressed: () => ref
                    .read(archivedSessionsProvider.notifier)
                    .restore(key),
              ),
            ),
          );
      },
      onTakeover: (Session session) =>
          unawaited(runTakeover(context, ref, session)),
    );
  }
}

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
    this.computers = const <ComputerHeading>[],
    this.now,
  });

  final List<ComputerHeading> computers;
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
                ProjectChip(
                  label: l10n.allProjects,
                  selected: project == null,
                  onTap: () => onProject(null),
                ),
                for (final String name in projects)
                  ProjectChip(
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

    final double stateHeight = math.max(
      320,
      MediaQuery.sizeOf(context).height * 0.5,
    );
    Widget fill(Widget child) => SizedBox(height: stateHeight, child: child);
    int rowIndex = 0;
    List<Widget> rows(Map<SessionState, List<Session>> groups) => _rows(context, groups, clock, () => rowIndex++);
    final bool perComputer = computers.length > 1;
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
    } else if (state.sessions.isEmpty && !perComputer) {
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
    } else if (groups.isEmpty && (!perComputer || query.isNotEmpty || project != null)) {
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
        if (!perComputer)
          ...rows(groups)
        else
          for (final ComputerHeading computer in computers) ...<Widget>[
            ComputerHeader(
              computer: computer,
              count: visible.where((Session s) => SessionRef.parse(s.id)?.host == computer.id).length,
            ),
            ...switch (groupSessions(visible.where((Session s) => SessionRef.parse(s.id)?.host == computer.id).toList(), query: query)) {
              final Map<SessionState, List<Session>> own when own.isEmpty => <Widget>[
                  Padding(
                    padding: const EdgeInsets.fromLTRB(Space.lg + Space.sm, Space.sm, Space.lg, 0),
                    child: Text(l10n.computerNoSessions, style: kit.meta.copyWith(color: kit.textMuted)),
                  ),
                ],
              final Map<SessionState, List<Session>> own => rows(own),
            },
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

  List<Widget> _rows(BuildContext context, Map<SessionState, List<Session>> groups, DateTime clock, int Function() nextIndex) {
      return <Widget>[
        for (final MapEntry<SessionState, List<Session>> group
            in groups.entries) ...<Widget>[
          SessionGroupHeader(state: group.key, count: group.value.length),
          for (final Session session in group.value) ...<Widget>[
            FlipOnMove(
              key: ValueKey<String>('row-${session.id}'),
              child: Entrance(
                delay: Duration(milliseconds: 40 * math.min(nextIndex(), 9)),
                rise: 12,
                child: SessionSwipeRow(
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
      ];
  }

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

void showSessionsTab(WidgetRef ref) =>
    ref.read(activeTabProvider.notifier).show(0);
