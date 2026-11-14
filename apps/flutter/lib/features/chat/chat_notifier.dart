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

class SendFailed extends SendState {
  const SendFailed({required this.text, this.viewOnly = false});
  final String text;
  final bool viewOnly;
}

sealed class TakeoverProgress {
  const TakeoverProgress();
}

class TakeoverIdle extends TakeoverProgress {
  const TakeoverIdle();
}

class TakeoverRunning extends TakeoverProgress {
  const TakeoverRunning();
}

class TakeoverFailed extends TakeoverProgress {
  const TakeoverFailed(this.error);
  final BridgeException error;
}

class PendingSend {
  const PendingSend({required this.id, required this.text, required this.time, this.failed = false, this.reason});
  final String id;
  final String text;
  final DateTime time;
  final bool failed;
  final BridgeException? reason;

  PendingSend asFailed(BridgeException? why) => PendingSend(id: id, text: text, time: time, failed: true, reason: why);
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
  final BridgeException? error;
  final SendState send;
  final BridgeException? answerError;
  final TakeoverProgress takeover;
  final List<PendingSend> pending;

  final bool refreshing;

  ChatState copyWith({
    LoadPhase? phase,
    List<ChatMessage>? messages,
    bool? hasMore,
    bool? loadingOlder,
    BridgeException? error,
    SendState? send,
    BridgeException? answerError,
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

class ChatNotifier extends Notifier<ChatState> {
  ChatNotifier(this.sessionId);
  final String sessionId;
  static const int pageSize = 50;

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
      if (error.isNotFound && sessionId.startsWith('pty:')) {
        state = state.copyWith(phase: LoadPhase.ready);
        return;
      }
      state = state.copyWith(phase: LoadPhase.error, error: error);
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

  static List<ChatMessage> _merge(List<ChatMessage> first, List<ChatMessage> second) {
    final Set<String> seen = <String>{};
    return <ChatMessage>[
      for (final ChatMessage m in <ChatMessage>[...first, ...second])
        if (seen.add(m.id)) m,
    ];
  }

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

  static List<PendingSend> _settle(List<PendingSend> pending, List<ChatMessage> seen) {
    if (pending.isEmpty) return pending;
    return <PendingSend>[
      for (final PendingSend p in pending)
        if (!seen.any((ChatMessage m) => m.role == ChatRole.user && m.text.trim() == p.text && m.time.isAfter(p.time.subtract(const Duration(seconds: 30))))) p,
    ];
  }

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
      if (error.isViewOnly) state = state.copyWith(send: SendFailed(text: entry.text, viewOnly: true));
      _markFailed(entry.id, error);
    }
  }

  void _markFailed(String id, BridgeException? reason) {
    if (!ref.mounted) return;
    state = state.copyWith(pending: <PendingSend>[for (final PendingSend p in state.pending) p.id == id ? p.asFailed(reason) : p]);
  }

  Future<void> retry(String id) async {
    final int index = state.pending.indexWhere((PendingSend p) => p.id == id);
    if (index < 0) return;
    final PendingSend fresh = PendingSend(id: id, text: state.pending[index].text, time: DateTime.now());
    state = state.copyWith(pending: <PendingSend>[for (final PendingSend p in state.pending) p.id == id ? fresh : p]);
    await _deliver(fresh);
  }

  void discard(String id) => state = state.copyWith(pending: <PendingSend>[for (final PendingSend p in state.pending) if (p.id != id) p]);

  Future<String?> takeOver({bool stopOriginal = false}) async {
    if (state.takeover is TakeoverRunning) return null;
    final BridgeClient? client = clientForSession(ref, sessionId);
    if (client == null) {
      state = state.copyWith(takeover: const TakeoverFailed(BridgeException('disconnected', '')));
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
      state = state.copyWith(takeover: TakeoverFailed(error));
      return null;
    }
  }

  void dismissTakeoverError() => state = state.copyWith(takeover: const TakeoverIdle());

  void dismissSendError() => state = state.copyWith(send: const SendIdle());

  Future<void> allow(PendingPrompt prompt, {required bool allow}) =>
      _answer((BridgeClient client) => client.promptAllow(prompt.requestId, allow: allow));

  Future<void> choose(PendingPrompt prompt, List<int> choices) =>
      _answer((BridgeClient client) => client.promptChoose(prompt.requestId, choices.cast<Object>()));

  Future<void> _answer(Future<void> Function(BridgeClient client) action) async {
    state = state.copyWith(clearAnswerError: true);
    final BridgeClient? client = clientForSession(ref, sessionId);
    if (client == null) {
      state = state.copyWith(answerError: const BridgeException('disconnected', ''));
      return;
    }
    try {
      await action(client);
    } on BridgeException catch (error) {
      if (!ref.mounted) return;
      state = state.copyWith(answerError: error);
    }
  }
}
