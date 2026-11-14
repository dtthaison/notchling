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

  Future<void> _subscribe(BridgeClient client) async {
    try {
      await client.subscribe(const SubscriptionSpec(sessions: true));
    } on BridgeException catch (_) {
    }
  }

  Future<void> _load(BridgeClient client) async {
    try {
      final List<Session> sessions = await client.listSessions();
      if (!ref.mounted) return;
      state = SessionsState(phase: LoadPhase.ready, sessions: sessions);
    } on BridgeException catch (error) {
      if (!ref.mounted) return;
      state = SessionsState(phase: LoadPhase.error, sessions: state.sessions, error: error);
    }
  }

  Future<void> refresh() async {
    if (ref.read(macsProvider).active?.id == hostId) return ref.read(sessionsProvider.notifier).refresh();
    final BridgeClient? client = ref.read(computerClientProvider(hostId));
    if (client != null) await _load(client);
  }
}

class ComputerSessions {
  const ComputerSessions({required this.computer, required this.scope, required this.status, required this.state});
  final MacProfile computer;

  final String scope;
  final ConnectionStatus status;

  final SessionsState state;
}

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

BridgeClient? clientForSession(Ref ref, String sessionId) {
  final SessionRef? picked = ref.read(selectedSessionProvider);
  if (picked != null && picked.id == sessionId && picked.host.isNotEmpty) return ref.read(computerClientProvider(picked.host));
  return ref.read(bridgeClientProvider);
}

final Provider<ChatCache> scopedChatCacheProvider = Provider<ChatCache>((Ref ref) {
  final String? active = ref.watch(macsProvider.select((MacsState s) => s.active?.id));
  final String scope = active == null ? '' : ref.watch(macsProvider.select((MacsState s) => s.scopeOf(active)));
  return ScopedChatCache(ref.watch(chatCacheProvider), scope);
});
