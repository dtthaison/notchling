import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:app_settings/app_settings.dart';

import '../../app/theme/tokens.dart';
import '../../app/widgets/common.dart';
import '../../core/protocol/models.dart';
import '../../l10n/app_localizations.dart';
import '../sessions/sessions_notifier.dart';
import '../sessions/status_style.dart';
import '../../app/shell.dart' show KeyboardLift, KeyboardLiftController;
import '../../app/alerts.dart';
import '../../debug/kb_trace.dart';
import '../voice/insertion.dart';
import 'takeover_flow.dart';
import '../voice/prompt_builder.dart';
import '../voice/voice_controller.dart';
import '../../app/notifications.dart' show lifecycleProvider;
import 'chat_notifier.dart';
import 'chat_pictures.dart';
import 'composer.dart';
import 'markdown_text.dart';
import 'pending_card.dart';

/// The Chat tab for the selected session.
class ChatScreen extends ConsumerWidget {
  const ChatScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final AppL10n l10n = AppL10n.of(context);
    final String? id = ref.watch(selectedSessionIdProvider);
    if (id == null) {
      return StateMessage(icon: Icons.chat_bubble_outline, title: l10n.chatSelectSession);
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
  bool _showNewPill = false;
  StreamSubscription<String>? _transcripts;
  late final VoiceController _voice;

  @override
  void initState() {
    super.initState();
    _voice = ref.read(voiceProvider.notifier);
    _transcripts = _voice.transcripts.listen((String text) => insertAtCursor(_controller, text));
    _scroll.addListener(() {
      if (_scroll.hasClients && _scroll.offset < 40 && _showNewPill) setState(() => _showNewPill = false);
    });
    // Opening the chat again: catch up on whatever was said while the phone was elsewhere.
    unawaited(Future<void>.microtask(() => mounted ? ref.read(chatProvider(widget.sessionId).notifier).refreshNewest() : null));
  }

  @override
  void dispose() {
    // A dictation belongs to the chat it started in: leaving drops it instead of typing into another one.
    unawaited(_transcripts?.cancel());
    unawaited(_voice.cancel());
    _controller.dispose();
    _lift.dispose();
    _scroll.dispose();
    super.dispose();
  }

  /// Confirms (twice for a running session whose original must be stopped), then asks the Mac to resume this
  /// session in its own terminal and switches to the result.
  Future<void> _takeOver() async {
    final Session? session = ref.read(sessionsProvider).byId(widget.sessionId);
    if (session == null) return;
    final TakeoverChoice choice = await confirmTakeover(context, session);
    if (choice == TakeoverChoice.cancel || !mounted) return;
    final String? newId = await ref.read(chatProvider(widget.sessionId).notifier).takeOver(stopOriginal: choice == TakeoverChoice.stopAndResume);
    if (newId != null && mounted) ref.read(selectedSessionIdProvider.notifier).select(newId);
  }

  /// Tap to start, tap to stop; a tap while transcribing cancels. The text goes into the composer, never out.
  Future<void> _toggleMic(Session? session, ChatState chat) async {
    final VoiceController voice = ref.read(voiceProvider.notifier);
    switch (ref.read(voiceProvider)) {
      case VoiceRecording():
        await voice.finish();
      case VoiceTranscribing():
        await voice.cancel();
      case VoiceIdle() || VoiceFailed():
        final String prompt = buildVoicePrompt(
          project: session?.project,
          recentTexts: chat.messages.reversed.take(20).toList().reversed.map((ChatMessage m) => '${m.text} ${m.toolSummary ?? ''}'),
        );
        await voice.start(prompt: prompt);
    }
  }

  Future<void> _send() async {
    final String text = _controller.text;
    final ChatNotifier notifier = ref.read(chatProvider(widget.sessionId).notifier);
    if (await notifier.send(text)) _controller.clear();
  }

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final String id = widget.sessionId;
    final Session? session = ref.watch(sessionsProvider.select((SessionsState s) => s.byId(id)));
    // Only what the list shows. The send state, takeover progress and dictation tick on their own (the voice
    // timer fires every 100 ms) and belong to the composer, so they must not rebuild the messages.
    final _ListData data = ref.watch(chatProvider(id).select((ChatState c) => _ListData(c.phase, c.messages, c.hasMore, c.loadingOlder, c.pending, c.refreshing)));
    ref.listen<AppLifecycleState>(lifecycleProvider, (AppLifecycleState? before, AppLifecycleState now) {
      if (now == AppLifecycleState.resumed) unawaited(ref.read(chatProvider(id).notifier).refreshNewest());
    });
    final String? answerError = ref.watch(chatProvider(id).select((ChatState c) => c.answerError));

    ref.listen<int>(chatProvider(id).select((ChatState c) => c.messages.length), (int? previous, int count) {
      if (count > _seenCount && _seenCount > 0 && _scroll.hasClients && _scroll.offset > 200) setState(() => _showNewPill = true);
      _seenCount = count;
    });

    final PendingPrompt? pending = session?.pending;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        if (session != null) _ChatHeader(session: session),
        const Divider(),
        _UpdatingLine(sessionId: id),
        Expanded(
          child: KbTracePointer(
            child: KeyboardLift(
              controller: _lift,
              list: _body(l10n, data),
              composer: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.stretch, children: <Widget>[
                if (pending != null)
                  PendingCard(
                    prompt: pending,
                    error: answerError,
                    onAllow: ({required bool allow}) => unawaited(ref.read(chatProvider(id).notifier).allow(pending, allow: allow)),
                    onChoose: (List<int> choices) => unawaited(ref.read(chatProvider(id).notifier).choose(pending, choices)),
                  ),
                Consumer(builder: (BuildContext context, WidgetRef ref, Widget? _) {
                  final SendState send = ref.watch(chatProvider(id).select((ChatState c) => c.send));
                  final TakeoverProgress takeoverProgress = ref.watch(chatProvider(id).select((ChatState c) => c.takeover));
                  return Composer(
                    controller: _controller,
                    send: send,
                    viewOnly: session != null && !session.canMessage || send is SendFailed && send.viewOnly,
                    onSend: () => unawaited(_send()),
                    onRetry: () {
                      final SendState failed = ref.read(chatProvider(id)).send;
                      if (failed is SendFailed) {
                        _controller.text = failed.text;
                        ref.read(chatProvider(id).notifier).dismissSendError();
                        unawaited(_send());
                      }
                    },
                    onDismissError: () => ref.read(chatProvider(id).notifier).dismissSendError(),
                    voice: ref.watch(voiceProvider),
                    onMic: () => unawaited(_toggleMic(session, ref.read(chatProvider(id)))),
                    onOpenSettings: () => unawaited(AppSettings.openAppSettings()),
                    onDismissVoice: () => ref.read(voiceProvider.notifier).dismissProblem(),
                    takeover: session?.takeover,
                    takeoverProgress: takeoverProgress,
                    onTakeover: () => unawaited(_takeOver()),
                  );
                }),
              ]),
            ),
          ),
        ),
      ],
    );
  }

  List<ChatMessage>? _reversedFor;
  List<ChatMessage> _newestFirst = const <ChatMessage>[];

  /// Newest-first copy of the messages, made again only when the list itself changed.
  List<ChatMessage> _newest(List<ChatMessage> messages) {
    if (!identical(_reversedFor, messages)) {
      _reversedFor = messages;
      _newestFirst = messages.reversed.toList(growable: false);
    }
    return _newestFirst;
  }

  Widget _body(AppL10n l10n, _ListData chat) {
    if (chat.phase == LoadPhase.loading && chat.messages.isEmpty) return const _ChatSkeleton();
    if (chat.phase == LoadPhase.error && chat.messages.isEmpty) {
      return ErrorState(message: l10n.loadFailed, onRetry: () => unawaited(ref.read(chatProvider(widget.sessionId).notifier).reload()));
    }
    if (chat.messages.isEmpty && chat.pending.isEmpty) return StateMessage(icon: Icons.chat_bubble_outline, title: l10n.chatEmpty);

    final List<ChatMessage> newestFirst = _newest(chat.messages);
    final List<PendingSend> waiting = chat.pending.reversed.toList(growable: false);
    // The room the keyboard has taken, once it has settled (see KeyboardLift): the newest message stays above the composer.
    return ValueListenableBuilder<double>(
      valueListenable: _lift.settled,
      builder: (BuildContext context, double room, Widget? _) => Stack(
      children: <Widget>[
        ListView.builder(
          controller: _scroll,
          reverse: true,
          padding: EdgeInsets.fromLTRB(Space.md, Space.md, Space.md, Space.md + room),
          itemCount: waiting.length + newestFirst.length + (chat.hasMore ? 1 : 0),
          itemBuilder: (BuildContext context, int position) {
            if (position < waiting.length) {
              final PendingSend entry = waiting[position];
              return Padding(
                padding: const EdgeInsets.only(bottom: Space.md),
                child: _PendingBubble(
                  key: ValueKey<String>(entry.id),
                  entry: entry,
                  onRetry: () => unawaited(ref.read(chatProvider(widget.sessionId).notifier).retry(entry.id)),
                  onDiscard: () => ref.read(chatProvider(widget.sessionId).notifier).discard(entry.id),
                ),
              );
            }
            final int index = position - waiting.length;
            if (index == newestFirst.length) {
              return Padding(
                padding: const EdgeInsets.only(bottom: Space.md),
                child: Center(
                  child: chat.loadingOlder
                      ? const SizedBox(width: 20, height: 20, child: CircularProgressIndicator(strokeWidth: 2))
                      : SecondaryButton(label: l10n.chatLoadOlder, onPressed: () => unawaited(ref.read(chatProvider(widget.sessionId).notifier).loadOlder())),
                ),
              );
            }
            final ChatMessage message = newestFirst[index];
            // Consecutive tool lines sit close together; other messages keep the full gap.
            final bool compact = message.role == ChatRole.tools && index > 0 && newestFirst[index - 1].role == ChatRole.tools;
            return Padding(
              padding: EdgeInsets.only(bottom: compact ? Space.xs : Space.md),
              child: RepaintBoundary(child: MessageBubble(key: ValueKey<String>(newestFirst[index].id), message: newestFirst[index], sessionId: widget.sessionId)),
            );
          },
        ),
        if (_showNewPill)
          Positioned(
            bottom: Space.sm + room,
            left: 0,
            right: 0,
            child: Center(
              child: SecondaryButton(
                label: l10n.chatNewMessages,
                icon: Icons.arrow_downward,
                onPressed: () {
                  unawaited(_scroll.animateTo(0, duration: reduceMotion(context) ? Duration.zero : const Duration(milliseconds: 200), curve: Curves.easeOut));
                  setState(() => _showNewPill = false);
                },
              ),
            ),
          ),
      ],
      ),
    );
  }
}

class _ChatHeader extends ConsumerWidget {
  const _ChatHeader({required this.session});
  final Session session;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final AppL10n l10n = AppL10n.of(context);
    final bool muted = ref.watch(alertPrefsProvider.select((AlertPrefs p) => p.muted.contains(session.id)));
    final String control = switch (session.control) {
      SessionControl.console => l10n.controlConsole,
      SessionControl.job => l10n.controlJob,
      SessionControl.viewOnly => l10n.viewOnly,
    };
    return Semantics(
      container: true,
      label: '${session.name}, ${session.project}, ${session.state.label(l10n)}, $control',
      child: Padding(
        padding: const EdgeInsets.fromLTRB(Space.lg, Space.md, Space.lg, Space.sm),
        child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: <Widget>[
          Row(children: <Widget>[
            StatusDot(state: session.state),
            const SizedBox(width: Space.sm),
            Expanded(child: Text(session.name, maxLines: 2, overflow: TextOverflow.ellipsis, style: TypeToken.titleMd)),
            if (session.state == SessionState.needsInput) const InputBadge(),
            IconButton(
              tooltip: muted ? l10n.unmuteSession : l10n.muteSession,
              constraints: const BoxConstraints(minWidth: Metric.minTarget, minHeight: Metric.minTarget),
              icon: Icon(muted ? Icons.notifications_off_outlined : Icons.notifications_none, size: 20, color: muted ? Token.statusInput : Token.muted),
              onPressed: () => unawaited(ref.read(alertPrefsProvider.notifier).setMuted(session.id, muted: !muted)),
            ),
          ]),
          const SizedBox(height: 2),
          Wrap(spacing: Space.md, children: <Widget>[
            Text(session.project, style: TypeToken.codeSm),
            Row(mainAxisSize: MainAxisSize.min, children: <Widget>[
              Icon(session.control == SessionControl.viewOnly ? Icons.visibility_outlined : Icons.terminal, size: 12, color: Token.muted),
              const SizedBox(width: 4),
              Text(control, style: TypeToken.labelSm),
            ]),
          ]),
        ]),
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
      child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: <Widget>[
        for (int i = 0; i < 4; i++)
          Align(
            alignment: i.isEven ? Alignment.centerRight : Alignment.centerLeft,
            child: Container(
              margin: const EdgeInsets.only(bottom: Space.md),
              width: 220.0 + i * 30,
              height: i.isEven ? 44 : 28,
              decoration: BoxDecoration(color: Token.surfaceRaised, borderRadius: BorderRadius.circular(Radii.lg)),
            ),
          ),
      ]),
    );
  }
}

/// One message: user right in a bubble, assistant left without one, tools as one mono expandable line.
class MessageBubble extends StatefulWidget {
  const MessageBubble({super.key, required this.message, required this.sessionId});
  final ChatMessage message;
  final String sessionId;

  @override
  State<MessageBubble> createState() => _MessageBubbleState();
}

class _MessageBubbleState extends State<MessageBubble> with SingleTickerProviderStateMixin {
  bool _expanded = false;
  late final AnimationController _enter = AnimationController(vsync: this, duration: const Duration(milliseconds: 200));

  @override
  void initState() {
    super.initState();
    // New messages slide up 8 px and fade in over 200 ms; Reduce Motion keeps only the fade. Only a message that
    // just arrived does this: bubbles built while scrolling or while the keyboard resizes the list stay still
    // (a fading bubble is drawn offscreen, which costs frames).
    final bool fresh = DateTime.now().difference(widget.message.time).inSeconds < 4;
    if (fresh) {
      unawaited(_enter.forward());
    } else {
      _enter.value = 1;
    }
  }

  @override
  void dispose() {
    _enter.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final ChatMessage message = widget.message;
    final Widget content = switch (message.role) {
      ChatRole.user => Align(
          alignment: Alignment.centerRight,
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: Metric.maxMessageWidth),
            child: Container(
              padding: const EdgeInsets.all(Space.md),
              decoration: BoxDecoration(color: Token.surfaceRaised, borderRadius: BorderRadius.circular(Radii.lg)),
              child: Column(crossAxisAlignment: CrossAxisAlignment.end, mainAxisSize: MainAxisSize.min, children: <Widget>[
                if (message.text.isNotEmpty) MarkdownText(message.text),
                MessagePictures(sessionId: widget.sessionId, images: message.images, alignEnd: true),
                if (message.queued) Padding(padding: const EdgeInsets.only(top: Space.xs), child: Text(l10n.chatQueued, style: TypeToken.labelSm)),
              ]),
            ),
          ),
        ),
      ChatRole.assistant => Align(
          alignment: Alignment.centerLeft,
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: Metric.maxMessageWidth),
            child: Column(crossAxisAlignment: CrossAxisAlignment.start, mainAxisSize: MainAxisSize.min, children: <Widget>[
              if (message.text.isNotEmpty) MarkdownText(message.text),
              MessagePictures(sessionId: widget.sessionId, images: message.images),
            ]),
          ),
        ),
      ChatRole.tools => Column(crossAxisAlignment: CrossAxisAlignment.start, mainAxisSize: MainAxisSize.min, children: <Widget>[
        Semantics(
          button: true,
          label: message.toolSummary ?? message.text,
          hint: _expanded ? l10n.toolsCollapse : l10n.toolsExpand,
          excludeSemantics: true,
          onTap: () => setState(() => _expanded = !_expanded),
          child: InkWell(
            onTap: () => setState(() => _expanded = !_expanded),
            child: ConstrainedBox(
              constraints: const BoxConstraints(minHeight: 32),
              child: Row(crossAxisAlignment: CrossAxisAlignment.center, children: <Widget>[
                Icon(_expanded ? Icons.expand_more : Icons.chevron_right, size: 16, color: Token.muted),
                const SizedBox(width: Space.sm),
                Expanded(
                  child: Text(message.toolSummary ?? message.text, maxLines: _expanded ? null : 1, overflow: TextOverflow.ellipsis, style: TypeToken.codeSm),
                ),
              ]),
            ),
          ),
        ),
        MessagePictures(sessionId: widget.sessionId, images: message.images),
      ]),
    };
    final bool still = reduceMotion(context);
    return AnimatedBuilder(
      animation: _enter,
      builder: (BuildContext context, Widget? child) => Opacity(
        opacity: Curves.easeOut.transform(_enter.value),
        child: Transform.translate(offset: Offset(0, still ? 0 : 8 * (1 - Curves.easeOut.transform(_enter.value))), child: child),
      ),
      child: content,
    );
  }
}

/// What the message list depends on. Equal when nothing it shows changed.
class _ListData {
  const _ListData(this.phase, this.messages, this.hasMore, this.loadingOlder, this.pending, this.refreshing);
  final List<PendingSend> pending;
  final bool refreshing;
  final LoadPhase phase;
  final List<ChatMessage> messages;
  final bool hasMore;
  final bool loadingOlder;

  @override
  bool operator ==(Object other) =>
      other is _ListData && other.phase == phase && identical(other.messages, messages) && other.hasMore == hasMore && other.loadingOlder == loadingOlder && identical(other.pending, pending);

  @override
  int get hashCode => Object.hash(phase, identityHashCode(messages), hasMore, loadingOlder, identityHashCode(pending));
}

/// A message just sent: shown at once on the right, with "đang gửi" until the Mac's transcript has it, or the
/// reason and a retry when the Mac did not take it.
class _PendingBubble extends StatelessWidget {
  const _PendingBubble({super.key, required this.entry, required this.onRetry, required this.onDiscard});
  final PendingSend entry;
  final VoidCallback onRetry;
  final VoidCallback onDiscard;

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    return Align(
      alignment: Alignment.centerRight,
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: Metric.maxMessageWidth),
        child: Opacity(
          opacity: entry.failed ? 1 : 0.7,
          child: Container(
            padding: const EdgeInsets.all(Space.md),
            decoration: BoxDecoration(
              color: Token.surfaceRaised,
              borderRadius: BorderRadius.circular(Radii.lg),
              border: entry.failed ? Border.all(color: Token.statusError) : null,
            ),
            child: Column(crossAxisAlignment: CrossAxisAlignment.end, mainAxisSize: MainAxisSize.min, children: <Widget>[
              MarkdownText(entry.text),
              const SizedBox(height: Space.xs),
              if (!entry.failed)
                Text(l10n.chatSending, style: TypeToken.labelSm)
              else
                Row(mainAxisSize: MainAxisSize.min, children: <Widget>[
                  Flexible(child: Text(entry.reason ?? l10n.chatSendFailed, style: TypeToken.labelSm.copyWith(color: Token.statusError))),
                  TextButton(onPressed: onRetry, child: Text(l10n.retry)),
                  IconButton(
                    tooltip: l10n.pictureClose,
                    constraints: const BoxConstraints(minWidth: 40, minHeight: 40),
                    icon: const Icon(Icons.close, size: 16, color: Token.muted),
                    onPressed: onDiscard,
                  ),
                ]),
            ]),
          ),
        ),
      ),
    );
  }
}

/// A thin line under the header while newer messages are being fetched. The messages already shown stay.
class _UpdatingLine extends ConsumerWidget {
  const _UpdatingLine({required this.sessionId});
  final String sessionId;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final bool busy = ref.watch(chatProvider(sessionId).select((ChatState c) => c.refreshing));
    if (!busy) return const SizedBox(height: 2);
    return Semantics(
      liveRegion: true,
      label: AppL10n.of(context).chatUpdating,
      child: const LinearProgressIndicator(minHeight: 2, backgroundColor: Colors.transparent),
    );
  }
}
