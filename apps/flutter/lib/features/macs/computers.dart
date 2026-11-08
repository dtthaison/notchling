import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/net/bridge_client.dart';
import '../../core/pairing/mac_profile.dart';
import '../../core/protocol/models.dart';
import '../chat/chat_cache.dart';
import '../connection/connection.dart';
import '../sessions/sessions_notifier.dart';
import 'macs_notifier.dart';
import 'session_ref.dart';

/// One paired computer's sessions, with bare ids. The computer in use shares [sessionsProvider], which Chat reads
/// and the subscription sync keeps; any other one is subscribed to its session list only.
final computerSessionsProvider = NotifierProvider.family<ComputerSessionsNotifier, SessionsState, String>(ComputerSessionsNotifier.new);

class ComputerSessionsNotifier extends Notifier<SessionsState> {
  ComputerSessionsNotifier(this.hostId);
  final String hostId;

  @override
  SessionsState build() {
    final bool inUse = ref.watch(macsProvider.select((MacsState s) => s.active?.id == hostId));
    if (inUse) return ref.watch(sessionsProvider);
    final BridgeClient? client = ref.watch(computerClientProvider(hostId));
    if (client == null) return const SessionsState();
    final StreamSubscription<BridgeEvent> events = client.events.listen((BridgeEvent event) {
      if (event is SessionsEvent) state = SessionsState(phase: LoadPhase.ready, sessions: event.sessions);
    });
    final StreamSubscription<ConnectionStatus> status = client.status.listen((ConnectionStatus next) {
      if (next.phase == LinkPhase.connected) unawaited(_load(client));
    });
    ref.onDispose(events.cancel);
    ref.onDispose(status.cancel);
    unawaited(_subscribe(client));
    if (client.isConnected) unawaited(_load(client));
    return const SessionsState();
  }

  /// Remembered by the client and sent again after each reconnect.
  Future<void> _subscribe(BridgeClient client) async {
    try {
      await client.subscribe(const SubscriptionSpec(sessions: true));
    } on BridgeException {
      // The list is still loaded on connect; pushes resume with the next reconnect.
    }
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

  Future<void> refresh() async {
    if (ref.read(macsProvider).active?.id == hostId) return ref.read(sessionsProvider.notifier).refresh();
    final BridgeClient? client = ref.read(computerClientProvider(hostId));
    if (client != null) await _load(client);
  }
}

/// A computer as the session list shows it: its heading, and its sessions with host-qualified ids.
class ComputerSessions {
  const ComputerSessions({required this.computer, required this.scope, required this.status, required this.state});
  final MacProfile computer;

  /// Its storage scope, for the archive (`MacsState.scopeOf`).
  final String scope;
  final ConnectionStatus status;

  /// Session ids here are `<host>/<id>` ([SessionRef.qualified]).
  final SessionsState state;
}

/// Every paired computer, in pairing order, each with its own sessions.
final Provider<List<ComputerSessions>> allComputersSessionsProvider = Provider<List<ComputerSessions>>((Ref ref) {
  final MacsState macs = ref.watch(macsProvider);
  return <ComputerSessions>[
    for (final MacProfile mac in macs.macs)
      ComputerSessions(
        computer: mac,
        scope: macs.scopeOf(mac.id),
        status: ref.watch(computerStatusProvider(mac.id)),
        state: qualifySessions(ref.watch(computerSessionsProvider(mac.id)), mac.id),
      ),
  ];
});

/// Every computer's sessions as one list (qualified ids): ready once any computer answered, an error only when
/// all of them failed.
SessionsState mergeComputerSessions(List<ComputerSessions> computers) {
  final List<SessionsState> states = <SessionsState>[for (final ComputerSessions c in computers) c.state];
  final List<Session> sessions = <Session>[for (final SessionsState s in states) ...s.sessions];
  if (states.any((SessionsState s) => s.phase == LoadPhase.ready)) return SessionsState(phase: LoadPhase.ready, sessions: sessions);
  if (states.isNotEmpty && states.every((SessionsState s) => s.phase == LoadPhase.error)) {
    return SessionsState(phase: LoadPhase.error, sessions: sessions, error: states.first.error);
  }
  return SessionsState(sessions: sessions);
}

SessionsState qualifySessions(SessionsState state, String host) => SessionsState(
      phase: state.phase,
      error: state.error,
      sessions: <Session>[for (final Session s in state.sessions) withSessionId(s, SessionRef(host, s.id).qualified)],
    );

/// The client an op on [sessionId] goes to: the picked session's own computer when this is the picked session,
/// else the computer in use. Never another computer that happens to have a session with the same id.
BridgeClient? clientForSession(Ref ref, String sessionId) {
  final SessionRef? picked = ref.read(selectedSessionProvider);
  if (picked != null && picked.id == sessionId && picked.host.isNotEmpty) return ref.read(computerClientProvider(picked.host));
  return ref.read(bridgeClientProvider);
}

/// The chat cache of the computer in use: its session ids carry that computer's storage scope.
final Provider<ChatCache> scopedChatCacheProvider = Provider<ChatCache>((Ref ref) {
  final String? active = ref.watch(macsProvider.select((MacsState s) => s.active?.id));
  final String scope = active == null ? '' : ref.watch(macsProvider.select((MacsState s) => s.scopeOf(active)));
  return ScopedChatCache(ref.watch(chatCacheProvider), scope);
});
