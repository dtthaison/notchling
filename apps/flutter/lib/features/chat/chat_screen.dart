import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:app_settings/app_settings.dart';

import '../../app/theme/tokens.dart';
import '../../app/widgets/common.dart';
import '../../core/protocol/models.dart';
import '../../l10n/app_localizations.dart';
import '../sessions/sessions_notifier.dart';
import '../../app/shell.dart' show KeyboardLift, KeyboardLiftController;
import '../../app/alerts.dart';
import '../../debug/kb_trace.dart';
import '../voice/insertion.dart';
import 'takeover_flow.dart';
import '../voice/prompt_builder.dart';
import '../voice/voice_controller.dart';
import '../../app/notifications.dart' show lifecycleProvider;
import '../../app/widgets/kit.dart';
import '../../app/widgets/motion.dart';
import '../macs/macs_notifier.dart';
import '../macs/macs_screen.dart';
import '../macs/session_ref.dart';
import 'chat_header.dart';
import 'photo_send.dart';
import 'chat_notifier.dart';
import 'chat_parts.dart';
import 'conversation_view.dart';
import 'slash_commands.dart';
import 'composer.dart';
import 'pending_card.dart';

class ChatScreen extends ConsumerWidget {
  const ChatScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final AppL10n l10n = AppL10n.of(context);
    final String? id = ref.watch(selectedSessionIdProvider);
    if (id == null) {
      return StateMessage(
        icon: Icons.chat_bubble_outline,
        title: l10n.chatSelectSession,
      );
    }
    return _SessionChat(key: ValueKey<String>(id), sessionId: id);
  }
}

class _SessionChat extends ConsumerStatefulWidget {
  const _SessionChat({super.key, required this.sessionId});
  final String sessionId;

  @override
  ConsumerState<_SessionChat> createState() => _SessionChatState();
}

class _SessionChatState extends ConsumerState<_SessionChat> {
  final TextEditingController _controller = TextEditingController();
  final ScrollController _scroll = ScrollController();
  final KeyboardLiftController _lift = KeyboardLiftController();
  int _seenCount = 0;
  bool _away = false;
  int _unread = 0;
  StreamSubscription<String>? _transcripts;
  late final VoiceController _voice;

  @override
  void initState() {
    super.initState();
    _voice = ref.read(voiceProvider.notifier);
    _transcripts = _voice.transcripts.listen(
      (String text) => insertAtCursor(_controller, text),
    );
    _scroll.addListener(() {
      if (!_scroll.hasClients) return;
      final bool away = _scroll.offset > 200;
      if (away != _away) {
        setState(() {
          _away = away;
          if (!away) _unread = 0;
        });
      }
    });
    unawaited(
      Future<void>.microtask(
        () => mounted
            ? ref.read(chatProvider(widget.sessionId).notifier).refreshNewest()
            : null,
      ),
    );
  }

  @override
  void dispose() {
    unawaited(_transcripts?.cancel());
    unawaited(_voice.cancel());
    _controller.dispose();
    _lift.dispose();
    _scroll.dispose();
    super.dispose();
  }

  Future<void> _takeOver() async {
    final Session? session = ref.read(sessionsProvider).byId(widget.sessionId);
    if (session == null) return;
    final String host = ref.read(selectedSessionProvider)?.host ?? ref.read(macsProvider).active?.id ?? '';
    final TakeoverChoice choice = await confirmTakeover(context, session);
    if (choice == TakeoverChoice.cancel || !mounted) return;
    final String? newId = await ref
        .read(chatProvider(widget.sessionId).notifier)
        .takeOver(stopOriginal: choice == TakeoverChoice.stopAndResume);
    if (newId != null && mounted) {
      ref.read(selectedSessionProvider.notifier).select(SessionRef(host, newId));
    }
  }

  Future<void> _toggleMic() async {
    final Session? session = ref.read(sessionsProvider).byId(widget.sessionId);
    final ChatState chat = ref.read(chatProvider(widget.sessionId));
    final VoiceController voice = ref.read(voiceProvider.notifier);
    switch (ref.read(voiceProvider)) {
      case VoiceRecording():
        await voice.finish();
      case VoiceTranscribing():
        await voice.cancel();
      case VoiceIdle() || VoiceFailed():
        final String prompt = buildVoicePrompt(
          project: session?.project,
          recentTexts: chat.messages.reversed
              .take(20)
              .toList()
              .reversed
              .map((ChatMessage m) => '${m.text} ${m.toolSummary ?? ''}'),
        );
        await voice.start(prompt: prompt);
    }
  }

  Future<void> _send() async {
    final String text = _controller.text;
    final ChatNotifier notifier = ref.read(
      chatProvider(widget.sessionId).notifier,
    );
    if (await notifier.send(text)) _controller.clear();
  }

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final String id = widget.sessionId;
    ref.watch(sessionsProvider.select((SessionsState s) => s.byId(id) != null));
    final _ListData data = ref.watch(
      chatProvider(id).select(
        (ChatState c) => _ListData(
          c.phase,
          c.messages,
          c.hasMore,
          c.loadingOlder,
          c.pending,
          c.refreshing,
        ),
      ),
    );
    ref.listen<AppLifecycleState>(lifecycleProvider, (
      AppLifecycleState? before,
      AppLifecycleState now,
    ) {
      if (now == AppLifecycleState.resumed) {
        unawaited(ref.read(chatProvider(id).notifier).refreshNewest());
      }
    });
    ref.listen<int>(
      chatProvider(id).select((ChatState c) => c.messages.length),
      (int? previous, int count) {
        if (count > _seenCount &&
            _seenCount > 0 &&
            _scroll.hasClients &&
            _scroll.offset > 200) {
          setState(() => _unread += count - _seenCount);
        }
        _seenCount = count;
      },
    );

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        _ChatHeader(sessionId: id),
        _UpdatingLine(sessionId: id),
        Expanded(
          child: KbTracePointer(
            child: KeyboardLift(
              controller: _lift,
              list: _body(l10n, data),
              composer: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: <Widget>[
                  SmoothSize(
                    alignment: Alignment.bottomCenter,
                    child: Consumer(
                      builder: (BuildContext context, WidgetRef ref, Widget? _) {
                        final PendingPrompt? pending = ref.watch(
                          sessionsProvider.select(
                            (SessionsState s) => s.byId(id)?.pending,
                          ),
                        );
                        final String? answerError = ref.watch(
                          chatProvider(id).select((ChatState c) => c.answerError),
                        );
                        if (pending == null) {
                          return const SizedBox(width: double.infinity);
                        }
                        return Entrance(
                          key: ValueKey<String>(pending.requestId),
                          rise: 16,
                          child: PendingCard(
                            prompt: pending,
                            error: answerError,
                            onAllow: ({required bool allow}) => unawaited(
                              ref.read(chatProvider(id).notifier).allow(pending, allow: allow),
                            ),
                            onChoose: (List<int> choices) => unawaited(
                              ref.read(chatProvider(id).notifier).choose(pending, choices),
                            ),
                          ),
                        );
                      },
                    ),
                  ),
                  Consumer(
                    builder: (BuildContext context, WidgetRef ref, Widget? _) {
                      final SendState send = ref.watch(
                        chatProvider(id).select((ChatState c) => c.send),
                      );
                      final TakeoverProgress takeoverProgress = ref.watch(
                        chatProvider(id).select((ChatState c) => c.takeover),
                      );
                      final ({
                        bool? canMessage,
                        String? cwd,
                        bool? takeoverAvailable,
                        bool? takeoverStoppable,
                        bool? takeoverBusy,
                        String? takeoverReason,
                      }) view = ref.watch(
                        sessionsProvider.select((SessionsState s) {
                          final Session? session = s.byId(id);
                          return (
                            canMessage: session?.canMessage,
                            cwd: session?.cwd,
                            takeoverAvailable: session?.takeover?.available,
                            takeoverStoppable: session?.takeover?.stoppable,
                            takeoverBusy: session?.takeover?.busy,
                            takeoverReason: session?.takeover?.reason,
                          );
                        }),
                      );
                      final bool hasSession = view.canMessage != null;
                      return Composer(
                        controller: _controller,
                        send: send,
                        viewOnly:
                            hasSession && view.canMessage == false ||
                            send is SendFailed && send.viewOnly,
                        onSend: () => unawaited(_send()),
                        onRetry: () {
                          final SendState failed = ref
                              .read(chatProvider(id))
                              .send;
                          if (failed is SendFailed) {
                            _controller.text = failed.text;
                            ref
                                .read(chatProvider(id).notifier)
                                .dismissSendError();
                            unawaited(_send());
                          }
                        },
                        onDismissError: () => ref
                            .read(chatProvider(id).notifier)
                            .dismissSendError(),
                        voice: ref.watch(voiceProvider),
                        onMic: () => unawaited(_toggleMic()),
                        onOpenSettings: () =>
                            unawaited(AppSettings.openAppSettings()),
                        onDismissVoice: () =>
                            ref.read(voiceProvider.notifier).dismissProblem(),
                        commands: ref.watch(slashCommandsProvider(id)),
                        folder: view.cwd,
                    onAttach: view.canMessage == true ? () => unawaited(pickAndSendPhoto(context, ref, id)) : null,
                        takeover: view.takeoverAvailable == null
                            ? null
                            : ref.read(sessionsProvider).byId(id)?.takeover,
                        takeoverProgress: takeoverProgress,
                        onTakeover: () => unawaited(_takeOver()),
                      );
                    },
                  ),
                ],
              ),
            ),
          ),
        ),
      ],
    );
  }

  List<ChatMessage>? _groupedFor;
  List<ChatEntry> _entries = const <ChatEntry>[];

  List<ChatEntry> _entriesOf(List<ChatMessage> messages) {
    if (!identical(_groupedFor, messages)) {
      _groupedFor = messages;
      _entries = groupEntries(messages);
    }
    return _entries;
  }

  Widget _body(AppL10n l10n, _ListData chat) {
    if (chat.phase == LoadPhase.loading && chat.messages.isEmpty) {
      return const _ChatSkeleton();
    }
    if (chat.phase == LoadPhase.error && chat.messages.isEmpty) {
      return ErrorState(
        message: l10n.loadFailed,
        onRetry: () => unawaited(
          ref.read(chatProvider(widget.sessionId).notifier).reload(),
        ),
      );
    }
    if (chat.messages.isEmpty && chat.pending.isEmpty) {
      return StateMessage(pet: 'wave-1', title: l10n.chatEmpty);
    }

    final ChatNotifier notifier = ref.read(
      chatProvider(widget.sessionId).notifier,
    );
    return ValueListenableBuilder<double>(
      valueListenable: _lift.settled,
      builder: (BuildContext context, double room, Widget? _) => Stack(
        children: <Widget>[
          ConversationList(
            sessionId: widget.sessionId,
            entries: _entriesOf(chat.messages),
            pending: chat.pending,
            hasMore: chat.hasMore,
            loadingOlder: chat.loadingOlder,
            onLoadOlder: () => unawaited(notifier.loadOlder()),
            onRetry: (String id) => unawaited(notifier.retry(id)),
            onDiscard: notifier.discard,
            controller: _scroll,
            room: room,
          ),
          Positioned(
            right: Space.lg,
            bottom: Space.md + room,
            child: ScrollDownButton(
              visible: _away,
              unread: _unread,
              onTap: () {
                unawaited(
                  _scroll.animateTo(0, duration: reduceMotion(context) ? Duration.zero : const Duration(milliseconds: 380), curve: Curves.easeOutCubic),
                );
                setState(() => _unread = 0);
              },
            ),
          ),
        ],
      ),
    );
  }
}

class _ChatHeader extends ConsumerWidget {
  const _ChatHeader({required this.sessionId});
  final String sessionId;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final Session? session = ref.watch(
      sessionsProvider.select((SessionsState s) => s.byId(sessionId)),
    );
    if (session == null) return const SizedBox.shrink();
    final String scope = ref.watch(macsProvider.select((MacsState s) => s.activeScope));
    final bool muted = ref.watch(
      alertPrefsProvider.select((AlertPrefs p) => p.isMuted(scope, session.id)),
    );
    return ChatHeaderView(
      session: session,
      muted: muted,
      onOpenMacs: () => unawaited(
        Navigator.of(context).push(
          MaterialPageRoute<void>(builder: (BuildContext context) => const MacsScreen()),
        ),
      ),
      onToggleMute: () => unawaited(
        ref
            .read(alertPrefsProvider.notifier)
            .setMuted(session.id, muted: !muted, scope: scope),
      ),
    );
  }
}

class _ChatSkeleton extends StatelessWidget {
  const _ChatSkeleton();

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(Space.md),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          for (int i = 0; i < 4; i++)
            Align(
              alignment: i.isEven
                  ? Alignment.centerRight
                  : Alignment.centerLeft,
              child: Container(
                margin: const EdgeInsets.only(bottom: Space.md),
                width: 220.0 + i * 30,
                height: i.isEven ? 44 : 28,
                decoration: BoxDecoration(
                  color: Kit.of(context).card,
                  borderRadius: BorderRadius.circular(Radii.md),
                ),
              ),
            ),
        ],
      ),
    );
  }
}

class _ListData {
  const _ListData(
    this.phase,
    this.messages,
    this.hasMore,
    this.loadingOlder,
    this.pending,
    this.refreshing,
  );
  final List<PendingSend> pending;
  final bool refreshing;
  final LoadPhase phase;
  final List<ChatMessage> messages;
  final bool hasMore;
  final bool loadingOlder;

  @override
  bool operator ==(Object other) =>
      other is _ListData &&
      other.phase == phase &&
      identical(other.messages, messages) &&
      other.hasMore == hasMore &&
      other.loadingOlder == loadingOlder &&
      identical(other.pending, pending);

  @override
  int get hashCode => Object.hash(
    phase,
    identityHashCode(messages),
    hasMore,
    loadingOlder,
    identityHashCode(pending),
  );
}

class _UpdatingLine extends ConsumerWidget {
  const _UpdatingLine({required this.sessionId});
  final String sessionId;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final bool busy = ref.watch(
      chatProvider(sessionId).select((ChatState c) => c.refreshing),
    );
    if (!busy) return const SizedBox(height: 2);
    return Semantics(
      liveRegion: true,
      label: AppL10n.of(context).chatUpdating,
      child: const LinearProgressIndicator(
        minHeight: 2,
        backgroundColor: Colors.transparent,
      ),
    );
  }
}

class ScrollDownButton extends StatelessWidget {
  const ScrollDownButton({super.key, required this.visible, required this.unread, required this.onTap});
  final bool visible;
  final int unread;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final Kit kit = Kit.of(context);
    final AppL10n l10n = AppL10n.of(context);
    return IgnorePointer(
      ignoring: !visible,
      child: AnimatedScale(
        scale: visible ? 1 : 0.6,
        duration: motionDuration(context, Motion.sheet),
        curve: Curves.easeOutBack,
        child: AnimatedOpacity(
          opacity: visible ? 1 : 0,
          duration: motionDuration(context, Motion.ui),
          child: Semantics(
            button: true,
            label: unread > 0 ? '${l10n.chatNewMessages}: $unread' : l10n.chatNewMessages,
            excludeSemantics: true,
            onTap: onTap,
            child: GestureDetector(
              behavior: HitTestBehavior.opaque,
              onTap: onTap,
              child: SizedBox(
                width: 52,
                height: 52,
                child: Center(
                  child: Stack(
                    clipBehavior: Clip.none,
                    children: <Widget>[
                      Surface(
                        level: 2,
                        fill: kit.card,
                        radius: BorderRadius.circular(Radii.full),
                        child: SizedBox(width: 40, height: 40, child: Icon(Icons.arrow_downward, size: 20, color: kit.text)),
                      ),
                      if (unread > 0)
                        Positioned(
                          right: -6,
                          top: -6,
                          child: Container(
                            constraints: const BoxConstraints(minWidth: 20, minHeight: 20),
                            padding: const EdgeInsets.symmetric(horizontal: 5),
                            alignment: Alignment.center,
                            decoration: BoxDecoration(color: kit.accent, borderRadius: BorderRadius.circular(Radii.full), border: Border.all(color: kit.window, width: 2)),
                            child: Text('$unread', style: kit.label.copyWith(color: kit.onAccent, fontSize: 11, fontWeight: FontWeight.w700)),
                          ),
                        ),
                    ],
                  ),
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}
