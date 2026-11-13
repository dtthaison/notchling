import 'package:flutter/material.dart';

import '../../app/theme/tokens.dart';
import '../../app/widgets/common.dart';
import '../../app/widgets/kit.dart';
import '../../app/widgets/motion.dart';
import '../../core/protocol/models.dart';
import '../../l10n/app_localizations.dart';
import 'chat_audio.dart';
import 'chat_pictures.dart';
import 'code_highlight.dart';
import 'markdown_text.dart';
import 'send_now.dart';

/// What the conversation list draws: one message, or a run of consecutive tool messages folded into one group.
sealed class ChatEntry {
  const ChatEntry();
  String get id;
}

class MessageEntry extends ChatEntry {
  const MessageEntry(this.message);
  final ChatMessage message;
  @override
  String get id => message.id;
}

class ToolGroupEntry extends ChatEntry {
  const ToolGroupEntry(this.messages);
  final List<ChatMessage> messages;
  @override
  String get id => 'tools:${messages.first.id}';

  List<ToolStep> get steps => <ToolStep>[
    for (final ChatMessage m in messages)
      if (m.steps.isNotEmpty)
        ...m.steps
      else
        ToolStep(
          name: m.toolSummary?.split(' ').first ?? 'tool',
          summary: m.toolSummary ?? m.text,
          output: m.toolSummary == null ? null : m.text,
        ),
  ];
}

/// Oldest to newest, tool runs folded.
List<ChatEntry> groupEntries(List<ChatMessage> messages) {
  final List<ChatEntry> entries = <ChatEntry>[];
  final List<ChatMessage> run = <ChatMessage>[];
  void flush() {
    if (run.isNotEmpty) entries.add(ToolGroupEntry(List<ChatMessage>.of(run)));
    run.clear();
  }

  for (final ChatMessage m in messages) {
    if (m.role == ChatRole.tools) {
      run.add(m);
    } else {
      flush();
      entries.add(MessageEntry(m));
    }
  }
  flush();
  return entries;
}

/// You: an accent-wash bubble on the right, 75% of the column at most.
class UserBubble extends StatelessWidget {
  const UserBubble({super.key, required this.message, required this.sessionId});
  final ChatMessage message;
  final String sessionId;

  @override
  Widget build(BuildContext context) {
    final Kit kit = Kit.of(context);
    final AppL10n l10n = AppL10n.of(context);
    return Align(
      alignment: Alignment.centerRight,
      child: FractionallySizedBox(
        widthFactor: 0.82,
        alignment: Alignment.centerRight,
        child: Align(
          alignment: Alignment.centerRight,
          child: Surface(
            level: 0,
            fill: kit.accentWash,
            radius: BorderRadius.only(topLeft: Radius.circular(Radii.bubble), topRight: Radius.circular(Radii.bubble), bottomLeft: Radius.circular(Radii.bubble), bottomRight: Radius.circular(Radii.bubbleTail)),
            padding: const EdgeInsets.symmetric(
              horizontal: Space.lg,
              vertical: Space.md,
            ),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.end,
              mainAxisSize: MainAxisSize.min,
              children: <Widget>[
                if (message.text.isNotEmpty) MarkdownText(message.text),
                MessagePictures(
                  sessionId: sessionId,
                  images: message.images,
                  alignEnd: true,
                ),
                if (message.queued) ...<Widget>[
                  Padding(
                    padding: const EdgeInsets.only(top: Space.xs),
                    child: Text(l10n.chatQueued, style: kit.meta),
                  ),
                  SendNowAction(sessionId: sessionId),
                ],
              ],
            ),
          ),
        ),
      ),
    );
  }
}

/// Claude: a card with the mark above the text.
class AssistantCard extends StatelessWidget {
  const AssistantCard({
    super.key,
    required this.message,
    required this.sessionId,
  });
  final ChatMessage message;
  final String sessionId;

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      width: double.infinity,
      child: Surface(
      level: 1,
      radius: BorderRadius.only(topLeft: Radius.circular(Radii.bubbleTail), topRight: Radius.circular(Radii.bubble), bottomLeft: Radius.circular(Radii.bubble), bottomRight: Radius.circular(Radii.bubble)),
      padding: const EdgeInsets.fromLTRB(
        Space.lg,
        Space.md,
        Space.lg,
        Space.lg,
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          const Padding(
            padding: EdgeInsets.only(bottom: Space.sm),
            child: KitClaudeMark(size: 22),
          ),
          if (message.text.isNotEmpty) MarkdownText(message.text),
          MessagePictures(sessionId: sessionId, images: message.images),
          MessageAudio(sessionId: sessionId, audio: message.audio),
        ],
      ),
      ),
    );
  }
}

/// A `!` command you sent: shell wash, mono, the `!` in the shell color; its output below.
class ShellBubble extends StatefulWidget {
  const ShellBubble({super.key, required this.message});
  final ChatMessage message;

  @override
  State<ShellBubble> createState() => _ShellBubbleState();
}

class _ShellBubbleState extends State<ShellBubble> {
  bool _all = false;
  static const int _previewLines = 8;

  @override
  Widget build(BuildContext context) {
    final Kit kit = Kit.of(context);
    final AppL10n l10n = AppL10n.of(context);
    final String command = widget.message.text.replaceFirst(
      RegExp(r'^\s*!'),
      '',
    );
    final List<String> output = (widget.message.output ?? '').trimRight().split(
      '\n',
    );
    final bool hasOutput = (widget.message.output ?? '').trim().isNotEmpty;
    final bool long = output.length > _previewLines;
    return Align(
      alignment: Alignment.centerRight,
      child: FractionallySizedBox(
        widthFactor: 0.92,
        alignment: Alignment.centerRight,
        child: Surface(
          level: 0,
          fill: kit.shellWash,
          radius: BorderRadius.only(topLeft: Radius.circular(Radii.bubble), topRight: Radius.circular(Radii.bubble), bottomLeft: Radius.circular(Radii.bubble), bottomRight: Radius.circular(Radii.bubbleTail)),
          padding: const EdgeInsets.all(Space.md),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            mainAxisSize: MainAxisSize.min,
            children: <Widget>[
              Text.rich(
                TextSpan(
                  children: <InlineSpan>[
                    TextSpan(
                      text: '! ',
                      style: kit.mono.copyWith(
                        color: kit.shell,
                        fontWeight: FontWeight.w700,
                      ),
                    ),
                    TextSpan(text: command, style: kit.mono),
                  ],
                ),
              ),
              if (hasOutput) ...<Widget>[
                const SizedBox(height: Space.sm),
                Container(
                  width: double.infinity,
                  padding: const EdgeInsets.all(Space.sm),
                  decoration: BoxDecoration(
                    color: kit.raised,
                    borderRadius: BorderRadius.circular(Radii.sm),
                  ),
                  child: Text(
                    (_all || !long ? output : output.take(_previewLines)).join(
                      '\n',
                    ),
                    style: kit.mono.copyWith(
                      fontSize: 12.5,
                      color: kit.textMuted,
                    ),
                  ),
                ),
                if (long && !_all)
                  TextButton(
                    onPressed: () => setState(() => _all = true),
                    child: Text('${l10n.showOutput} (${output.length})'),
                  ),
              ],
            ],
          ),
        ),
      ),
    );
  }
}

/// One notice that the conversation was compacted: hairlines, an icon, the title and the summary line.
class CompactionNotice extends StatelessWidget {
  const CompactionNotice({super.key, required this.message});
  final ChatMessage message;

  @override
  Widget build(BuildContext context) {
    final Kit kit = Kit.of(context);
    final AppL10n l10n = AppL10n.of(context);
    return Semantics(
      container: true,
      label: '${l10n.compactionTitle}. ${message.text}',
      child: Column(
        children: <Widget>[
          Row(
            children: <Widget>[
              Expanded(child: Divider(color: kit.hairline)),
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: Space.sm),
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: <Widget>[
                    Icon(Icons.unfold_less, size: 14, color: kit.textMuted),
                    const SizedBox(width: 6),
                    Text(
                      l10n.compactionTitle,
                      style: kit.label.copyWith(color: kit.textMuted),
                    ),
                  ],
                ),
              ),
              Expanded(child: Divider(color: kit.hairline)),
            ],
          ),
          if (message.text.isNotEmpty)
            Padding(
              padding: const EdgeInsets.only(top: Space.xs),
              child: Text(
                message.text,
                textAlign: TextAlign.center,
                style: kit.meta,
              ),
            ),
        ],
      ),
    );
  }
}

/// A run of tool calls as one raised capsule ("7 tool steps"); tap to open the steps, tap a step for its output or diff.
class ToolGroupView extends StatefulWidget {
  const ToolGroupView({
    super.key,
    required this.entry,
    this.initiallyOpen = false,
    this.sessionId,
  });
  final ToolGroupEntry entry;
  final bool initiallyOpen;

  /// Needed to load the group's pictures; without it they are not shown.
  final String? sessionId;

  @override
  State<ToolGroupView> createState() => _ToolGroupViewState();
}

class _ToolGroupViewState extends State<ToolGroupView> {
  late bool _open = widget.initiallyOpen;

  @override
  Widget build(BuildContext context) {
    final Kit kit = Kit.of(context);
    final AppL10n l10n = AppL10n.of(context);
    final List<ToolStep> steps = widget.entry.steps;
    final String title = steps.length == 1
        ? l10n.toolStepOne
        : l10n.toolStepsCount(steps.length);
    final ToolStep last = steps.last;
    final bool edits = steps.any((ToolStep s) => s.isEdit);
    final List<ChatImage> pictures = <ChatImage>[
      for (final ChatMessage m in widget.entry.messages) ...m.images,
    ];
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        Semantics(
          button: true,
          expanded: _open,
          label: '$title, ${last.summary}',
          excludeSemantics: true,
          onTap: () => setState(() => _open = !_open),
          child: InkWell(
            borderRadius: BorderRadius.circular(Radii.md),
            onTap: () => setState(() => _open = !_open),
            child: ConstrainedBox(
              constraints: const BoxConstraints(minHeight: Metric.minTarget),
              child: Surface(
              level: 1,
              fill: kit.raised,
              padding: const EdgeInsets.symmetric(
                horizontal: Space.md,
                vertical: Space.sm,
              ),
              child: Row(
                children: <Widget>[
                  Icon(
                    edits ? Icons.edit_note : Icons.build_outlined,
                    size: 16,
                    color: kit.textMuted,
                  ),
                  const SizedBox(width: Space.sm),
                  Flexible(flex: 0, child: Text(title, maxLines: 1, softWrap: false, overflow: TextOverflow.ellipsis, style: kit.label)),
                  const SizedBox(width: Space.sm),
                  Expanded(
                    child: Text(
                      last.summary,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: kit.mono.copyWith(
                        fontSize: 12,
                        color: kit.textMuted,
                      ),
                    ),
                  ),
                  AnimatedRotation(
                    turns: _open ? 0.5 : 0,
                    duration: motionDuration(context),
                    curve: Motion.out,
                    child: Icon(
                      Icons.expand_more,
                      size: 20,
                      color: kit.textMuted,
                    ),
                  ),
                ],
              ),
              ),
            ),
          ),
        ),
        ExpandReveal(
          expanded: _open,
          child: Padding(
            padding: const EdgeInsets.only(top: Space.xs),
            child: Column(
              children: <Widget>[
                for (final ToolStep step in steps)
                  _StepRow(
                    step: step,
                    initiallyOpen: widget.initiallyOpen && step.isEdit,
                  ),
              ],
            ),
          ),
        ),
        // A tool's pictures (a screenshot, an image it read) stay outside the fold, as Chat on the Mac shows them.
        if (pictures.isNotEmpty && widget.sessionId != null)
          MessagePictures(sessionId: widget.sessionId!, images: pictures),
      ],
    );
  }
}

class _StepRow extends StatefulWidget {
  const _StepRow({required this.step, this.initiallyOpen = false});
  final ToolStep step;
  final bool initiallyOpen;

  @override
  State<_StepRow> createState() => _StepRowState();
}

class _StepRowState extends State<_StepRow> {
  late bool _open = widget.initiallyOpen;

  @override
  Widget build(BuildContext context) {
    final Kit kit = Kit.of(context);
    final ToolStep step = widget.step;
    final bool expandable =
        step.isEdit || (step.output != null && step.output!.trim().isNotEmpty);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        InkWell(
          onTap: expandable ? () => setState(() => _open = !_open) : null,
          child: Container(
            constraints: const BoxConstraints(minHeight: 40),
            padding: const EdgeInsets.symmetric(
              horizontal: Space.sm,
              vertical: Space.xs,
            ),
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.center,
              children: <Widget>[
                Container(
                  padding: const EdgeInsets.symmetric(
                    horizontal: 6,
                    vertical: 1,
                  ),
                  decoration: BoxDecoration(
                    color: step.isEdit ? kit.addWash : kit.raised,
                    borderRadius: BorderRadius.circular(Radii.sm),
                  ),
                  child: Text(
                    step.name,
                    style: kit.mono.copyWith(
                      fontSize: 12,
                      color: step.isEdit ? kit.add : kit.textMuted,
                    ),
                  ),
                ),
                const SizedBox(width: Space.sm),
                Expanded(
                  child: Text(
                    step.summary,
                    maxLines: _open ? 4 : 1,
                    overflow: TextOverflow.ellipsis,
                    style: kit.mono.copyWith(fontSize: 12.5),
                  ),
                ),
                if (expandable)
                  AnimatedRotation(
                    turns: _open ? 0.25 : 0,
                    duration: motionDuration(context),
                    curve: Motion.out,
                    child: Icon(
                      Icons.chevron_right,
                      size: 18,
                      color: kit.textFaint,
                    ),
                  ),
              ],
            ),
          ),
        ),
        ExpandReveal(
          expanded: _open && (step.isEdit || expandable),
          child: step.isEdit
              ? DiffView(path: step.path, lines: step.diff)
              : Container(
                  width: double.infinity,
                  margin: const EdgeInsets.only(bottom: Space.xs),
                  padding: const EdgeInsets.all(Space.sm),
                  decoration: BoxDecoration(
                    color: kit.raised,
                    borderRadius: BorderRadius.circular(Radii.sm),
                  ),
                  child: Text(
                    (step.output ?? '').trimRight(),
                    maxLines: 14,
                    overflow: TextOverflow.fade,
                    style: kit.mono.copyWith(
                      fontSize: 12,
                      color: kit.textMuted,
                    ),
                  ),
                ),
        ),
      ],
    );
  }
}

/// An Edit or Write: the path, then the changed lines with a gutter, added on green and removed on red, highlighted.
class DiffView extends StatelessWidget {
  const DiffView({super.key, required this.lines, this.path});
  final List<DiffLine> lines;
  final String? path;

  @override
  Widget build(BuildContext context) {
    final Kit kit = Kit.of(context);
    final String? lang = languageOf(path);
    final int added = lines
        .where((DiffLine l) => l.kind == DiffKind.added)
        .length;
    final int removed = lines
        .where((DiffLine l) => l.kind == DiffKind.removed)
        .length;
    return Container(
      margin: const EdgeInsets.only(bottom: Space.xs),
      decoration: kitCard(kit, radius: Radii.sm, fill: kit.raised),
      clipBehavior: Clip.antiAlias,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: <Widget>[
          Padding(
            padding: const EdgeInsets.fromLTRB(
              Space.md,
              Space.sm,
              Space.md,
              Space.sm,
            ),
            child: Row(
              children: <Widget>[
                Expanded(
                  child: Text(
                    path ?? '',
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: kit.mono.copyWith(fontSize: 12),
                  ),
                ),
                Text(
                  '+$added',
                  style: kit.mono.copyWith(fontSize: 12, color: kit.add),
                ),
                const SizedBox(width: Space.sm),
                Text(
                  '-$removed',
                  style: kit.mono.copyWith(fontSize: 12, color: kit.danger),
                ),
              ],
            ),
          ),
          SingleChildScrollView(
            scrollDirection: Axis.horizontal,
            child: IntrinsicWidth(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: <Widget>[
                  for (final DiffLine line in lines)
                    Container(
                      color: switch (line.kind) {
                        DiffKind.added => kit.addWash,
                        DiffKind.removed => kit.removeWash,
                        DiffKind.context => null,
                      },
                      padding: const EdgeInsets.symmetric(
                        horizontal: Space.sm,
                        vertical: 1,
                      ),
                      child: Row(
                        children: <Widget>[
                          SizedBox(
                            width: 16,
                            child: Text(
                              switch (line.kind) {
                                DiffKind.added => '+',
                                DiffKind.removed => '-',
                                DiffKind.context => ' ',
                              },
                              style: kit.mono.copyWith(
                                fontSize: 12.5,
                                color: line.kind == DiffKind.added
                                    ? kit.add
                                    : (line.kind == DiffKind.removed
                                          ? kit.danger
                                          : kit.textFaint),
                              ),
                            ),
                          ),
                          Text.rich(
                            TextSpan(
                              children: highlightCode(
                                line.text,
                                kit,
                                language: lang,
                              ),
                            ),
                            style: kit.mono.copyWith(fontSize: 12.5),
                          ),
                        ],
                      ),
                    ),
                  const SizedBox(height: Space.xs),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }
}

/// The reply you just sent, until the Mac's transcript has it.
class PendingBubble extends StatelessWidget {
  const PendingBubble({
    super.key,
    required this.text,
    required this.failed,
    required this.reason,
    required this.onRetry,
    required this.onDiscard,
  });
  final String text;
  final bool failed;
  final String? reason;
  final VoidCallback onRetry;
  final VoidCallback onDiscard;

  @override
  Widget build(BuildContext context) {
    final Kit kit = Kit.of(context);
    final AppL10n l10n = AppL10n.of(context);
    final bool shell = RegExp(r'^\s*!').hasMatch(text);
    final BorderRadius radius = BorderRadius.only(topLeft: Radius.circular(Radii.bubble), topRight: Radius.circular(Radii.bubble), bottomLeft: Radius.circular(Radii.bubble), bottomRight: Radius.circular(Radii.bubbleTail));
    // The bubble is drawn exactly like the UserBubble that replaces it (same fill, padding, opacity, text, 82% at
    // most). "Sending" sits beside it, not inside or under it: the list is anchored at the bottom, so any height that
    // goes away when the Mac's transcript has the message would move the bubble. Only a failure, which needs its
    // buttons, goes underneath.
    return LayoutBuilder(
      builder: (BuildContext context, BoxConstraints box) {
        final Widget bubble = ConstrainedBox(
          constraints: BoxConstraints(maxWidth: box.maxWidth * 0.82),
          child: Surface(
            level: 0,
            fill: shell ? kit.shellWash : kit.accentWash,
            radius: radius,
            outline: failed ? kit.danger : null,
            padding: const EdgeInsets.symmetric(horizontal: Space.lg, vertical: Space.md),
            child: shell ? Text(text, style: kit.mono) : MarkdownText(text),
          ),
        );
        if (!failed) {
          return Row(
            mainAxisAlignment: MainAxisAlignment.end,
            crossAxisAlignment: CrossAxisAlignment.end,
            children: <Widget>[
              Flexible(
                child: Text(l10n.chatSending, style: kit.meta, textAlign: TextAlign.end, maxLines: 1, overflow: TextOverflow.ellipsis),
              ),
              const SizedBox(width: Space.sm),
              bubble,
            ],
          );
        }
        return Column(
          crossAxisAlignment: CrossAxisAlignment.end,
          mainAxisSize: MainAxisSize.min,
          children: <Widget>[
            bubble,
            Row(
              mainAxisSize: MainAxisSize.min,
              children: <Widget>[
                Flexible(
                  child: Text(
                    reason ?? l10n.chatSendFailed,
                    style: kit.meta.copyWith(color: kit.danger),
                  ),
                ),
                TextButton(onPressed: onRetry, child: Text(l10n.retry)),
                IconButton(
                  onPressed: onDiscard,
                  tooltip: l10n.close,
                  icon: Icon(Icons.close, size: 18, color: kit.textMuted),
                ),
              ],
            ),
          ],
        );
      },
    );
  }
}

/// A day or time marker is not needed on a phone; spacing carries the rhythm.
EdgeInsets entryGap(ChatEntry entry, ChatEntry? newer) => EdgeInsets.only(
  bottom: entry is ToolGroupEntry && newer is ToolGroupEntry
      ? Space.xs
      : Space.md,
);

/// Reduce Motion aware fade for new entries is handled by the list; kept here so previews can import one place.
bool reducedMotion(BuildContext context) => reduceMotion(context);

/// Under the newest message while the session runs: three dots that rise in turn, and what it is doing. One controller,
/// stopped when the tab is hidden; with Reduce Motion the dots stay level.
class WorkingIndicator extends StatefulWidget {
  const WorkingIndicator({super.key, this.activity});
  final String? activity;

  @override
  State<WorkingIndicator> createState() => _WorkingIndicatorState();
}

class _WorkingIndicatorState extends State<WorkingIndicator> with SingleTickerProviderStateMixin {
  late final AnimationController _wave = AnimationController(vsync: this, duration: const Duration(milliseconds: 1100));

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final bool run = !MediaQuery.disableAnimationsOf(context) && MediaQuery.viewInsetsOf(context).bottom == 0 && TickerMode.valuesOf(context).enabled;
    if (run && !_wave.isAnimating) {
      _wave.repeat();
    } else if (!run) {
      _wave.stop();
    }
  }

  @override
  void dispose() {
    _wave.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final Kit kit = Kit.of(context);
    final AppL10n l10n = AppL10n.of(context);
    return Semantics(
      liveRegion: true,
      label: widget.activity ?? l10n.stateRunning,
      excludeSemantics: true,
      child: Padding(
        padding: const EdgeInsets.fromLTRB(Space.sm, 0, Space.sm, 0),
        child: Row(
          children: <Widget>[
            RepaintBoundary(
              child: AnimatedBuilder(
                animation: _wave,
                builder: (BuildContext context, Widget? _) => Row(
                  mainAxisSize: MainAxisSize.min,
                  children: <Widget>[
                    for (int i = 0; i < 3; i++)
                      Padding(
                        padding: const EdgeInsets.only(right: 4),
                        child: Transform.translate(
                          offset: Offset(0, -4 * _bump((_wave.value - i * 0.16) % 1.0)),
                          child: Container(
                            width: 7,
                            height: 7,
                            decoration: BoxDecoration(color: kit.working.withValues(alpha: 0.55 + 0.45 * _bump((_wave.value - i * 0.16) % 1.0)), shape: BoxShape.circle),
                          ),
                        ),
                      ),
                  ],
                ),
              ),
            ),
            const SizedBox(width: Space.sm),
            Expanded(
              child: AnimatedSwitcher(
                duration: motionDuration(context),
                child: Text(
                  widget.activity ?? l10n.stateRunning,
                  key: ValueKey<String?>(widget.activity),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: kit.meta,
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }

  /// 0 at rest, 1 at the top of the hop: a smooth bump over the first third of the cycle.
  static double _bump(double t) => t < 0.33 ? Curves.easeInOut.transform(1 - (t / 0.33 - 1).abs()) : 0;
}
