
import 'package:flutter/material.dart';

import '../../app/theme/tokens.dart';
import '../../app/widgets/common.dart';
import '../../core/protocol/models.dart';
import '../../l10n/app_localizations.dart';
import '../../app/widgets/motion.dart';
import 'chat_notifier.dart';
import 'chat_parts.dart';

class ConversationList extends StatelessWidget {
  const ConversationList({
    super.key,
    required this.sessionId,
    required this.entries,
    required this.pending,
    required this.hasMore,
    required this.loadingOlder,
    required this.onLoadOlder,
    required this.onRetry,
    required this.onDiscard,
    this.controller,
    this.room = 0,
    this.openGroups = const <String>{},
    this.working = false,
    this.activity,
  });

  final String sessionId;
  final List<ChatEntry> entries;
  final List<PendingSend> pending;
  final bool hasMore;
  final bool loadingOlder;
  final VoidCallback onLoadOlder;
  final void Function(String) onRetry;
  final void Function(String) onDiscard;
  final ScrollController? controller;

  final double room;

  final Set<String> openGroups;

  final bool working;
  final String? activity;

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final List<PendingSend> waiting = pending.reversed.toList(growable: false);
    final List<ChatEntry> newestFirst = entries.reversed.toList(
      growable: false,
    );
    return ListView.builder(
      controller: controller,
      reverse: true,
      padding: EdgeInsets.fromLTRB(
        Space.md,
        Space.md,
        Space.md,
        Space.md + room,
      ),
      itemCount: (working ? 1 : 0) + waiting.length + newestFirst.length + (hasMore ? 1 : 0),
      itemBuilder: (BuildContext context, int rawPosition) {
        if (working && rawPosition == 0) {
          return Padding(padding: const EdgeInsets.only(bottom: Space.md), child: WorkingIndicator(activity: activity));
        }
        final int position = rawPosition - (working ? 1 : 0);
        if (position < waiting.length) {
          final PendingSend entry = waiting[position];
          return Padding(
            padding: const EdgeInsets.only(bottom: Space.md),
            child: PendingBubble(
              key: ValueKey<String>(entry.id),
              text: entry.text,
              failed: entry.failed,
              reason: entry.reason,
              onRetry: () => onRetry(entry.id),
              onDiscard: () => onDiscard(entry.id),
            ),
          );
        }
        final int index = position - waiting.length;
        if (index == newestFirst.length) {
          return Padding(
            padding: const EdgeInsets.only(bottom: Space.md),
            child: Center(
              child: loadingOlder
                  ? const SizedBox(
                      width: 20,
                      height: 20,
                      child: CircularProgressIndicator(strokeWidth: 2),
                    )
                  : SecondaryButton(
                      label: l10n.chatLoadOlder,
                      onPressed: onLoadOlder,
                    ),
            ),
          );
        }
        final ChatEntry entry = newestFirst[index];
        final ChatEntry? newer = index > 0 ? newestFirst[index - 1] : null;
        return Padding(
          padding: entryGap(entry, newer),
          child: RepaintBoundary(
            child: ChatEntryView(
              key: ValueKey<String>(entry.id),
              entry: entry,
              sessionId: sessionId,
              open: openGroups.contains(entry.id),
            ),
          ),
        );
      },
    );
  }
}

class ChatEntryView extends StatefulWidget {
  const ChatEntryView({
    super.key,
    required this.entry,
    required this.sessionId,
    this.open = false,
  });
  final ChatEntry entry;
  final String sessionId;
  final bool open;

  @override
  State<ChatEntryView> createState() => _ChatEntryViewState();
}

class _ChatEntryViewState extends State<ChatEntryView> {
  late final bool _fresh = switch (widget.entry) {
    MessageEntry(:final ChatMessage message) => DateTime.now().difference(message.time).inSeconds < 4,
    ToolGroupEntry(:final List<ChatMessage> messages) => DateTime.now().difference(messages.last.time).inSeconds < 4,
  };

  @override
  Widget build(BuildContext context) {
    final ChatEntry entry = widget.entry;
    final Widget content = switch (entry) {
      ToolGroupEntry() => ToolGroupView(entry: entry, initiallyOpen: widget.open),
      MessageEntry(:final ChatMessage message) => switch ((message.role, message.kind)) {
        (ChatRole.user, ChatKind.shell) => ShellBubble(message: message),
        (ChatRole.user, _) => UserBubble(message: message, sessionId: widget.sessionId),
        (_, ChatKind.compaction) => CompactionNotice(message: message),
        _ => AssistantCard(message: message, sessionId: widget.sessionId),
      },
    };
    if (!_fresh) return content;
    final bool mine = entry is MessageEntry && entry.message.role == ChatRole.user;
    return Entrance(offset: mine ? const Offset(28, 16) : const Offset(-20, 16), child: content);
  }
}
