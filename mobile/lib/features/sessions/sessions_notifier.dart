import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/net/bridge_client.dart';
import '../../core/protocol/models.dart';
import '../connection/connection.dart';

enum LoadPhase { loading, ready, error }

class SessionsState {
  const SessionsState({this.phase = LoadPhase.loading, this.sessions = const <Session>[], this.error});
  final LoadPhase phase;
  final List<Session> sessions;
  final String? error;

  Session? byId(String? id) {
    if (id == null) return null;
    for (final Session s in sessions) {
      if (s.id == id) return s;
    }
    return null;
  }
}

/// Sessions grouped the way the list shows them, in the order Cần input, Đang chạy, Xong, Đã thoát.
/// `query` filters by name, project and subagent text; newest activity first inside a group.
Map<SessionState, List<Session>> groupSessions(List<Session> sessions, {String query = ''}) {
  final String needle = query.trim().toLowerCase();
  bool matches(Session s) =>
      needle.isEmpty ||
      s.name.toLowerCase().contains(needle) ||
      s.project.toLowerCase().contains(needle) ||
      s.subagents.any((Subagent a) => a.description.toLowerCase().contains(needle) || a.type.toLowerCase().contains(needle));
  final Map<SessionState, List<Session>> groups = <SessionState, List<Session>>{
    for (final SessionState state in SessionState.values) state: <Session>[],
  };
  for (final Session s in sessions.where(matches)) {
    groups[s.state]!.add(s);
  }
  for (final List<Session> group in groups.values) {
    group.sort((Session a, Session b) => b.lastActivity.compareTo(a.lastActivity));
  }
  return <SessionState, List<Session>>{
    for (final MapEntry<SessionState, List<Session>> e in groups.entries)
      if (e.value.isNotEmpty) e.key: e.value,
  };
}

final NotifierProvider<SessionsNotifier, SessionsState> sessionsProvider = NotifierProvider<SessionsNotifier, SessionsState>(SessionsNotifier.new);

/// The live session list: loaded when the link comes up, then kept current by the Mac's pushes.
class SessionsNotifier extends Notifier<SessionsState> {
  @override
  SessionsState build() {
    final BridgeClient? client = ref.watch(bridgeClientProvider);
    ref.watch(connectedCountProvider);
    if (client == null) return const SessionsState();

    final StreamSubscription<BridgeEvent> events = client.events.listen((BridgeEvent event) {
      switch (event) {
        case SessionsEvent(:final List<Session> sessions):
          state = SessionsState(phase: LoadPhase.ready, sessions: sessions);
        case PromptEvent():
          break;
        default:
          break;
      }
    });
    ref.onDispose(events.cancel);
    if (client.isConnected) unawaited(_load(client));
    return const SessionsState();
  }

  Future<void> _load(BridgeClient client) async {
    try {
      final List<Session> sessions = await client.listSessions();
      if (!ref.mounted) return;
      state = SessionsState(phase: LoadPhase.ready, sessions: sessions);
    } on BridgeException catch (error) {
      if (!ref.mounted) return;
      state = SessionsState(phase: LoadPhase.error, sessions: state.sessions, error: error.message);
    }
  }

  /// Pull to refresh, and the retry button.
  Future<void> refresh() async {
    final BridgeClient? client = ref.read(bridgeClientProvider);
    if (client == null) return;
    if (state.phase == LoadPhase.error) state = SessionsState(sessions: state.sessions);
    await _load(client);
  }
}

final NotifierProvider<SelectedSessionNotifier, String?> selectedSessionIdProvider =
    NotifierProvider<SelectedSessionNotifier, String?>(SelectedSessionNotifier.new);

/// The session the Chat and Visualize tabs show. Sends go to exactly this id.
class SelectedSessionNotifier extends Notifier<String?> {
  @override
  String? build() => null;

  void select(String? id) => state = id;
}
