import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/net/bridge_client.dart';
import '../../core/protocol/models.dart';
import '../connection/connection.dart';
import '../macs/computers.dart';
import '../sessions/sessions_notifier.dart';
import 'chat_cache.dart';

sealed class SendState {
  const SendState();
}

class SendIdle extends SendState {
  const SendIdle();
}

class SendSending extends SendState {
  const SendSending();
}

/// [viewOnly] marks the refusal of a session the Mac cannot type into (nothing was sent).
class SendFailed extends SendState {
  const SendFailed({required this.text, this.viewOnly = false, this.message});
  final String text;
  final bool viewOnly;
  final String? message;
}

/// Progress of "Tiếp quản" on this chat's session.
sealed class TakeoverProgress {
  const TakeoverProgress();
}

class TakeoverIdle extends TakeoverProgress {
  const TakeoverIdle();
}

class TakeoverRunning extends TakeoverProgress {
  const TakeoverRunning();
}

/// [message] is the Mac's own reason (busy, no transcript, ...).
class TakeoverFailed extends TakeoverProgress {
  const TakeoverFailed(this.message);
  final String message;
}

/// A message the user just sent, shown at once while the Mac takes it. It goes away when the same text shows up
/// in the transcript; if the Mac could not take it, it stays with [failed] and a retry.
class PendingSend {
  const PendingSend({required this.id, required this.text, required this.time, this.failed = false, this.reason});
  final String id;
  final String text;
  final DateTime time;
  final bool failed;
  final String? reason;

  PendingSend asFailed(String? why) => PendingSend(id: id, text: text, time: time, failed: true, reason: why);
}

class ChatState {
  const ChatState({
    this.phase = LoadPhase.loading,
    this.messages = const <ChatMessage>[],
    this.hasMore = false,
    this.loadingOlder = false,
    this.error,
    this.send = const SendIdle(),
    this.answerError,
    this.takeover = const TakeoverIdle(),
    this.pending = const <PendingSend>[],
    this.refreshing = false,
  });

  final LoadPhase phase;
  final List<ChatMessage> messages;
  final bool hasMore;
  final bool loadingOlder;
  final String? error;
  final SendState send;
  final String? answerError;
  final TakeoverProgress takeover;
  final List<PendingSend> pending;

  /// Newer messages are being fetched while the ones already here stay on screen.
  final bool refreshing;

  ChatState copyWith({
    LoadPhase? phase,
    List<ChatMessage>? messages,
    bool? hasMore,
    bool? loadingOlder,
    String? error,
    SendState? send,
    String? answerError,
    bool clearAnswerError = false,
    TakeoverProgress? takeover,
    List<PendingSend>? pending,
    bool? refreshing,
  }) =>
      ChatState(
        phase: phase ?? this.phase,
        messages: messages ?? this.messages,
        hasMore: hasMore ?? this.hasMore,
        loadingOlder: loadingOlder ?? this.loadingOlder,
        error: error,
        send: send ?? this.send,
        answerError: clearAnswerError ? null : (answerError ?? this.answerError),
        takeover: takeover ?? this.takeover,
        pending: pending ?? this.pending,
        refreshing: refreshing ?? this.refreshing,
      );
}

final chatProvider =
    NotifierProvider.family<ChatNotifier, ChatState, String>(ChatNotifier.new);

/// The chat of one session. The id it was built with is the id every send goes to: the screen shows
/// the selected session, so a message can never land in another one.
class ChatNotifier extends Notifier<ChatState> {
  ChatNotifier(this.sessionId);
  final String sessionId;
  static const int pageSize = 50;

  /// What this build reads from: the computer in use and its cache. A page that comes back after the computer
  /// changed is dropped, so one computer's messages never show under another's session.
  BridgeClient? _client;
  late ChatCache _cache;

  @override
  ChatState build() {
    final BridgeClient? client = ref.watch(bridgeClientProvider);
    ref.watch(connectedCountProvider);
    _client = client;
    _cache = ref.watch(scopedChatCacheProvider);
    if (client != null) {
      final StreamSubscription<BridgeEvent> events = client.events.listen((BridgeEvent event) {
        if (event is ChatEvent && event.sessionId == sessionId) _append(event.messages);
      });
      ref.onDispose(events.cancel);
    }
    unawaited(_start(client));
    return const ChatState();
  }

  /// Shows what the phone kept from last time at once, then asks the Mac only for what is newer. With nothing
  /// kept, the first page is fetched (the screen shows its skeleton until then).
  Future<void> _start(BridgeClient? client) async {
    final List<ChatMessage> kept = await _cache.loadMessages(sessionId);
    if (!ref.mounted || !identical(client, _client)) return;
    if (kept.isNotEmpty) {
      client?.rememberNewest(sessionId, kept.last.id);
      state = state.copyWith(phase: LoadPhase.ready, messages: _dedupe(_merge(kept, state.messages)), hasMore: true);
    }
    if (client == null || !client.isConnected) return;
    if (kept.isEmpty) {
      await _loadFirstPage(client);
    } else {
      await refreshNewest();
    }
  }

  void _persist() => unawaited(_cache.saveMessages(sessionId, state.messages));

  Future<void> _loadFirstPage(BridgeClient client) async {
    try {
      final ({List<ChatMessage> messages, bool hasMore, bool reset}) page = await client.chatGet(sessionId, limit: pageSize);
      if (!ref.mounted || !identical(client, _client)) return;
      state = state.copyWith(phase: LoadPhase.ready, messages: _dedupe(_merge(page.messages, state.messages)), hasMore: page.hasMore);
      _persist();
    } on BridgeException catch (error) {
      if (!ref.mounted) return;
      // A Console terminal that has not become a Claude session yet has no transcript to load.
      if (error.isNotFound && sessionId.startsWith('pty:')) {
        state = state.copyWith(phase: LoadPhase.ready);
        return;
      }
      state = state.copyWith(phase: LoadPhase.error, error: error.message);
    }
  }

  Future<void> reload() async {
    final BridgeClient? client = ref.read(bridgeClientProvider);
    if (client == null) return;
    state = state.copyWith(phase: LoadPhase.loading);
    await _loadFirstPage(client);
  }

  Future<void> loadOlder() async {
    final BridgeClient? client = ref.read(bridgeClientProvider);
    if (client == null || state.loadingOlder || !state.hasMore || state.messages.isEmpty) return;
    state = state.copyWith(loadingOlder: true);
    try {
      final ({List<ChatMessage> messages, bool hasMore, bool reset}) page =
          await client.chatGet(sessionId, limit: pageSize, before: state.messages.first.id);
      if (!ref.mounted || !identical(client, _client)) return;
      state = state.copyWith(messages: _merge(page.messages, state.messages), hasMore: page.hasMore, loadingOlder: false);
    } on BridgeException {
      if (!ref.mounted) return;
      state = state.copyWith(loadingOlder: false);
    }
  }

  void _append(List<ChatMessage> fresh) {
    final List<ChatMessage> merged = _dedupe(_merge(state.messages, fresh));
    state = state.copyWith(messages: merged, pending: _settle(state.pending, fresh));
    _persist();
  }

  /// Joins two ordered lists, dropping ids already present.
  static List<ChatMessage> _merge(List<ChatMessage> first, List<ChatMessage> second) {
    final Set<String> seen = <String>{};
    return <ChatMessage>[
      for (final ChatMessage m in <ChatMessage>[...first, ...second])
        if (seen.add(m.id)) m,
    ];
  }

  /// A message that was waiting in the agent's queue is replaced by the same text once it has been taken.
  static List<ChatMessage> _dedupe(List<ChatMessage> messages) {
    final Set<String> delivered = <String>{
      for (final ChatMessage m in messages)
        if (m.role == ChatRole.user && !m.queued) m.text.trim(),
    };
    return <ChatMessage>[
      for (final ChatMessage m in messages)
        if (!(m.queued && delivered.contains(m.text.trim()))) m,
    ];
  }

  /// Pending sends that now appear in the transcript (as a normal or a waiting message) are done.
  static List<PendingSend> _settle(List<PendingSend> pending, List<ChatMessage> seen) {
    if (pending.isEmpty) return pending;
    return <PendingSend>[
      for (final PendingSend p in pending)
        if (!seen.any((ChatMessage m) => m.role == ChatRole.user && m.text.trim() == p.text && m.time.isAfter(p.time.subtract(const Duration(seconds: 30))))) p,
    ];
  }

  /// Asks the Mac for the messages after the newest one the phone has and merges them in (nothing on screen is
  /// cleared meanwhile). Used when the chat is opened again, after a reconnect and when the app comes back to the
  /// front: replies that arrived while the phone was elsewhere are not pushed to it afterwards. When the Mac can
  /// no longer find that message, or more came than one page holds, the newest page replaces what is shown.
  Future<void> refreshNewest() async {
    final BridgeClient? client = ref.read(bridgeClientProvider);
    if (client == null || !client.isConnected || state.refreshing || state.phase == LoadPhase.loading) return;
    final Iterable<ChatMessage> settled = state.messages.where((ChatMessage m) => !m.queued);
    if (settled.isEmpty) {
      await _loadFirstPage(client);
      return;
    }
    state = state.copyWith(refreshing: true);
    try {
      final ({List<ChatMessage> messages, bool hasMore, bool reset}) page = await client.chatGet(sessionId, limit: pageSize, after: settled.last.id);
      if (!ref.mounted || !identical(client, _client)) return;
      final bool replace = page.reset || page.hasMore;
      state = state.copyWith(
        phase: LoadPhase.ready,
        messages: replace ? _dedupe(page.messages) : _dedupe(_merge(state.messages, page.messages)),
        hasMore: replace ? true : state.hasMore,
        refreshing: false,
        pending: _settle(state.pending, page.messages),
      );
      _persist();
    } on BridgeException catch (error) {
      debugPrint('ChatNotifier.refreshNewest($sessionId): ${error.code} ${error.message}');
      if (!ref.mounted) return;
      state = state.copyWith(refreshing: false);
    }
  }

  int _localCounter = 0;

  /// Sends [text] to this chat's session. The message appears in the list at once (waiting for the Mac), and the
  /// caller can clear its field. A view-only session is refused here, before anything leaves the phone. Returns
  /// false when the text was not taken (the caller keeps it in the field).
  Future<bool> send(String text) async {
    final String trimmed = text.trim();
    if (trimmed.isEmpty) return false;
    final Session? session = ref.read(sessionsProvider).byId(sessionId);
    if (session != null && !session.canMessage) {
      state = state.copyWith(send: SendFailed(text: trimmed, viewOnly: true));
      return false;
    }
    final PendingSend entry = PendingSend(id: 'local:${_localCounter++}', text: trimmed, time: DateTime.now());
    state = state.copyWith(send: const SendIdle(), pending: <PendingSend>[...state.pending, entry]);
    unawaited(_deliver(entry));
    return true;
  }

  Future<void> _deliver(PendingSend entry) async {
    final BridgeClient? client = clientForSession(ref, sessionId);
    if (client == null) {
      _markFailed(entry.id, null);
      return;
    }
    try {
      await client.sessionMessage(sessionId, entry.text);
    } on BridgeException catch (error) {
      if (!ref.mounted) return;
      if (error.isViewOnly) state = state.copyWith(send: SendFailed(text: entry.text, viewOnly: true, message: error.message));
      _markFailed(entry.id, error.message);
    }
  }

  void _markFailed(String id, String? reason) {
    if (!ref.mounted) return;
    state = state.copyWith(pending: <PendingSend>[for (final PendingSend p in state.pending) p.id == id ? p.asFailed(reason) : p]);
  }

  /// Sends a failed message again; it goes back to "đang gửi".
  Future<void> retry(String id) async {
    final int index = state.pending.indexWhere((PendingSend p) => p.id == id);
    if (index < 0) return;
    final PendingSend fresh = PendingSend(id: id, text: state.pending[index].text, time: DateTime.now());
    state = state.copyWith(pending: <PendingSend>[for (final PendingSend p in state.pending) p.id == id ? fresh : p]);
    await _deliver(fresh);
  }

  void discard(String id) => state = state.copyWith(pending: <PendingSend>[for (final PendingSend p in state.pending) if (p.id != id) p]);

  /// Asks the Mac to resume this view-only session in a terminal it owns. Returns the id of the session
  /// to switch to, or null when the Mac refused (the reason is in [ChatState.takeover]).
  Future<String?> takeOver({bool stopOriginal = false}) async {
    if (state.takeover is TakeoverRunning) return null;
    final BridgeClient? client = clientForSession(ref, sessionId);
    if (client == null) {
      state = state.copyWith(takeover: const TakeoverFailed('Chưa kết nối tới Mac.'));
      return null;
    }
    state = state.copyWith(takeover: const TakeoverRunning());
    try {
      final String newId = await client.sessionTakeover(sessionId, stopOriginal: stopOriginal);
      if (!ref.mounted) return newId;
      state = state.copyWith(takeover: const TakeoverIdle());
      return newId;
    } on BridgeException catch (error) {
      if (!ref.mounted) return null;
      state = state.copyWith(takeover: TakeoverFailed(error.message));
      return null;
    }
  }

  void dismissTakeoverError() => state = state.copyWith(takeover: const TakeoverIdle());

  void dismissSendError() => state = state.copyWith(send: const SendIdle());

  Future<void> allow(PendingPrompt prompt, {required bool allow}) =>
      _answer((BridgeClient client) => client.promptAllow(prompt.requestId, allow: allow));

  /// [choices] has one entry per question: the 1-based option number.
  Future<void> choose(PendingPrompt prompt, List<int> choices) =>
      _answer((BridgeClient client) => client.promptChoose(prompt.requestId, choices.cast<Object>()));

  /// Answers go to this session's own computer ([clientForSession]).
  Future<void> _answer(Future<void> Function(BridgeClient client) action) async {
    state = state.copyWith(clearAnswerError: true);
    final BridgeClient? client = clientForSession(ref, sessionId);
    if (client == null) {
      state = state.copyWith(answerError: 'Chưa kết nối tới Mac.');
      return;
    }
    try {
      await action(client);
    } on BridgeException catch (error) {
      if (!ref.mounted) return;
      state = state.copyWith(answerError: error.message);
    }
  }
}
