import 'package:flutter/material.dart';

import '../../app/theme/tokens.dart';
import '../../app/widgets/common.dart';
import '../../l10n/app_localizations.dart';
import '../../core/protocol/models.dart';
import '../voice/mic_button.dart';
import '../voice/voice_controller.dart';
import 'chat_notifier.dart';
import '../../app/widgets/kit.dart';
import '../../app/widgets/motion.dart';
import 'shell_command.dart';
import 'slash_commands.dart';

class Composer extends StatefulWidget {
  const Composer({
    super.key,
    required this.controller,
    required this.send,
    required this.viewOnly,
    required this.onSend,
    required this.onRetry,
    required this.onDismissError,
    this.readOnlyReason,
    this.takeover,
    this.takeoverProgress = const TakeoverIdle(),
    this.onTakeover,
    this.voice = const VoiceIdle(),
    this.onMic,
    this.onOpenSettings,
    this.onDismissVoice,
    this.focusNode,
    this.commands = const <SlashCommand>[],
    this.folder,
    this.onAttach,
  });

  final TextEditingController controller;
  final SendState send;
  final bool viewOnly;

  final String? readOnlyReason;

  final TakeoverInfo? takeover;
  final TakeoverProgress takeoverProgress;
  final VoidCallback? onTakeover;

  final VoiceState voice;
  final VoidCallback? onMic;
  final VoidCallback? onOpenSettings;
  final VoidCallback? onDismissVoice;
  final FocusNode? focusNode;
  final VoidCallback onSend;
  final VoidCallback onRetry;
  final VoidCallback onDismissError;

  final List<SlashCommand> commands;
  final String? folder;

  final VoidCallback? onAttach;

  @override
  State<Composer> createState() => _ComposerState();
}

class _ComposerState extends State<Composer> {
  FocusNode? _own;

  List<SlashCommand> _lastSuggestions = const <SlashCommand>[];
  FocusNode get _focus => widget.focusNode ?? (_own ??= FocusNode());

  @override
  void initState() {
    super.initState();
    _focus.addListener(_changed);
  }

  @override
  void dispose() {
    _focus.removeListener(_changed);
    _own?.dispose();
    super.dispose();
  }

  void _changed() {
    if (mounted) setState(() {});
  }

  void _pick(SlashCommand command) {
    final String text = '/${command.name} ';
    widget.controller.value = TextEditingValue(
      text: text,
      selection: TextSelection.collapsed(offset: text.length),
    );
    _focus.requestFocus();
  }

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final Kit kit = Kit.of(context);
    final SendState state = widget.send;
    final bool locked = widget.viewOnly || widget.readOnlyReason != null;
    return Container(
      decoration: BoxDecoration(
        color: kit.window,
        border: Border(top: BorderSide(color: kit.hairline)),
      ),
      padding: const EdgeInsets.fromLTRB(
        Space.md,
        Space.sm,
        Space.md,
        Space.sm,
      ),
      child: SafeArea(
        top: false,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: <Widget>[
            if (state is SendFailed && !state.viewOnly)
              Padding(
                padding: const EdgeInsets.only(bottom: Space.sm),
                child: Row(
                  children: <Widget>[
                    Icon(Icons.error_outline, size: 16, color: kit.danger),
                    const SizedBox(width: Space.sm),
                    Expanded(
                      child: Text(
                        l10n.sendFailed,
                        style: kit.bodySm.copyWith(color: kit.danger),
                      ),
                    ),
                    TextButton(
                      onPressed: widget.onRetry,
                      child: Text(l10n.retry),
                    ),
                    IconButton(
                      onPressed: widget.onDismissError,
                      icon: const Icon(Icons.close, size: 18),
                      tooltip: l10n.close,
                    ),
                  ],
                ),
              ),
            if (!locked)
              _VoiceNotice(
                voice: widget.voice,
                onOpenSettings: widget.onOpenSettings,
                onDismiss: widget.onDismissVoice,
              ),
            if (widget.viewOnly &&
                widget.readOnlyReason == null &&
                widget.takeover != null)
              _TakeoverRow(
                info: widget.takeover!,
                progress: widget.takeoverProgress,
                onTakeover: widget.onTakeover,
              )
            else if (locked)
              _LockedRow(text: widget.readOnlyReason ?? l10n.viewOnlyReason)
            else
              ValueListenableBuilder<TextEditingValue>(
                valueListenable: widget.controller,
                builder:
                    (
                      BuildContext context,
                      TextEditingValue value,
                      Widget? child,
                    ) => _box(context, kit, l10n, value.text),
              ),
          ],
        ),
      ),
    );
  }

  Widget _box(BuildContext context, Kit kit, AppL10n l10n, String text) {
    final bool sending = widget.send is SendSending;
    final bool shell = ShellCommand.isCommand(text);
    final String? slashTyped =
        text.startsWith('/') && !text.contains(RegExp(r'\s'))
        ? text.substring(1)
        : null;
    final List<SlashCommand> suggestions = slashTyped == null
        ? const <SlashCommand>[]
        : matchSlashCommands(slashTyped, widget.commands, limit: 5);
    final String? willRun =
        shell && ShellCommand.normalize(text) != text.trimLeft()
        ? ShellCommand.normalize(text).substring(1)
        : null;
    if (suggestions.isNotEmpty) _lastSuggestions = suggestions;
    final bool focused = _focus.hasFocus;
    final bool empty = text.trim().isEmpty || (shell && text.trim() == '!');
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        ExpandReveal(
          expanded: suggestions.isNotEmpty,
          alignment: Alignment.bottomCenter,
          child: _SlashList(
            commands: suggestions.isNotEmpty ? suggestions : _lastSuggestions,
            onPick: _pick,
          ),
        ),
        TweenAnimationBuilder<Color?>(
          tween: ColorTween(end: shell ? kit.shellWash : kit.card),
          duration: motionDuration(context),
          curve: Motion.out,
          builder: (BuildContext context, Color? fill, Widget? child) => Surface(
            level: 2,
            fill: fill,
            radius: BorderRadius.circular(Radii.composer),
            ring: shell
                ? kit.shell.withValues(alpha: 0.7)
                : (focused ? kit.accent.withValues(alpha: 0.55) : null),
            padding: const EdgeInsets.fromLTRB(
              Space.xs,
              Space.xs,
              Space.xs,
              Space.xs,
            ),
            child: child!,
          ),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: <Widget>[
              ExpandReveal(
                expanded: shell,
                duration: Motion.ui,
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: <Widget>[
                    Padding(
                      padding: const EdgeInsets.fromLTRB(
                        Space.md,
                        Space.sm,
                        Space.md,
                        0,
                      ),
                      child: Row(
                        children: <Widget>[
                          Icon(Icons.terminal, size: 16, color: kit.shell),
                          const SizedBox(width: Space.sm),
                          Text(
                            l10n.shellHeader,
                            style: kit.label.copyWith(color: kit.shell),
                          ),
                        ],
                      ),
                    ),
                    Padding(
                      padding: const EdgeInsets.fromLTRB(
                        Space.md,
                        2,
                        Space.md,
                        0,
                      ),
                      child: Text(
                        l10n.shellRunsIn(widget.folder ?? '~'),
                        style: kit.meta,
                      ),
                    ),
                  ],
                ),
              ),
              ExpandReveal(
                expanded: willRun != null,
                duration: Motion.ui,
                child: Container(
                  margin: const EdgeInsets.fromLTRB(
                    Space.md,
                    Space.sm,
                    Space.md,
                    0,
                  ),
                  padding: const EdgeInsets.all(Space.sm),
                  decoration: BoxDecoration(
                    color: kit.raised,
                    borderRadius: BorderRadius.circular(Radii.sm),
                  ),
                  child: Text.rich(
                    TextSpan(
                      children: <InlineSpan>[
                        TextSpan(
                          text: '${l10n.shellWillRun}  ',
                          style: kit.meta,
                        ),
                        TextSpan(
                          text: willRun ?? '',
                          style: kit.mono.copyWith(fontSize: 12.5),
                        ),
                      ],
                    ),
                  ),
                ),
              ),
              TextField(
                controller: widget.controller,
                focusNode: _focus,
                enabled: !sending,
                minLines: 1,
                maxLines: 6,
                keyboardType: TextInputType.multiline,
                textInputAction: TextInputAction.newline,
                style: shell ? kit.mono.copyWith(fontSize: 14.5) : kit.body,
                cursorColor: shell ? kit.shell : kit.accent,
                decoration: InputDecoration(
                  hintText: l10n.composerHint,
                  filled: false,
                  border: InputBorder.none,
                  enabledBorder: InputBorder.none,
                  focusedBorder: InputBorder.none,
                  disabledBorder: InputBorder.none,
                  contentPadding: const EdgeInsets.fromLTRB(
                    Space.md,
                    Space.md,
                    Space.md,
                    Space.xs,
                  ),
                ),
              ),
              Row(
                children: <Widget>[
                  if (widget.onAttach != null)
                    IconButton(
                      onPressed: sending ? null : widget.onAttach,
                      tooltip: l10n.attachPhoto,
                      constraints: const BoxConstraints(
                        minWidth: Metric.minTarget,
                        minHeight: Metric.minTarget,
                      ),
                      icon: Icon(
                        Icons.add_photo_alternate_outlined,
                        size: 22,
                        color: kit.textMuted,
                      ),
                    ),
                  MicButton(state: widget.voice, onPressed: widget.onMic),
                  const Spacer(),
                  Padding(
                    padding: const EdgeInsets.all(2),
                    child: SendDisc(
                      label: sending ? l10n.sending : l10n.send,
                      shell: shell,
                      enabled: !sending && !empty,
                      onPressed: widget.onSend,
                    ),
                  ),
                ],
              ),
              ExpandReveal(
                expanded: shell,
                duration: Motion.ui,
                child: Padding(
                  padding: const EdgeInsets.fromLTRB(
                    Space.md,
                    0,
                    Space.md,
                    Space.sm,
                  ),
                  child: SizedBox(
                    width: double.infinity,
                    child: Text(l10n.shellHint, style: kit.meta),
                  ),
                ),
              ),
            ],
          ),
        ),
      ],
    );
  }
}

class SendDisc extends StatelessWidget {
  const SendDisc({
    super.key,
    required this.label,
    required this.shell,
    required this.enabled,
    required this.onPressed,
  });
  final String label;
  final bool shell;
  final bool enabled;
  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) {
    final Kit kit = Kit.of(context);
    return Semantics(
      button: true,
      enabled: enabled,
      label: label,
      excludeSemantics: true,
      onTap: enabled ? onPressed : null,
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: enabled ? onPressed : null,
        child: PressScale(
          enabled: enabled,
          child: SizedBox(
            width: Metric.minTarget - 4,
            height: Metric.minTarget - 4,
            child: Center(
              child: AnimatedContainer(
                duration: motionDuration(context),
                curve: Motion.out,
                width: 40,
                height: 40,
                decoration: BoxDecoration(
                  color: enabled
                      ? (shell ? kit.shell : kit.accent)
                      : kit.raised,
                  shape: BoxShape.circle,
                ),
                child: AnimatedSwitcher(
                  duration: motionDuration(context, Motion.ui),
                  transitionBuilder: (Widget child, Animation<double> a) =>
                      RotationTransition(
                        turns: Tween<double>(begin: 0.25, end: 0).animate(a),
                        child: ScaleTransition(
                          scale: a,
                          child: FadeTransition(opacity: a, child: child),
                        ),
                      ),
                  child: Icon(
                    shell ? Icons.play_arrow_rounded : Icons.arrow_upward,
                    key: ValueKey<String>(
                      '${shell ? 'run' : 'send'}-${enabled ? 'on' : 'off'}',
                    ),
                    size: 22,
                    color: enabled
                        ? (shell ? kit.onShell : kit.onAccent)
                        : kit.textFaint,
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

class _SlashList extends StatelessWidget {
  const _SlashList({required this.commands, required this.onPick});
  final List<SlashCommand> commands;
  final void Function(SlashCommand) onPick;

  @override
  Widget build(BuildContext context) {
    final Kit kit = Kit.of(context);
    final AppL10n l10n = AppL10n.of(context);
    return Padding(
      padding: const EdgeInsets.only(bottom: Space.sm),
      child: Surface(
        level: 2,
        radius: BorderRadius.circular(Radii.md),
        clip: true,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: <Widget>[
            Padding(
              padding: const EdgeInsets.fromLTRB(
                Space.md,
                Space.sm,
                Space.md,
                Space.xs,
              ),
              child: Text(l10n.slashHeader, style: kit.section),
            ),
            for (final (int index, SlashCommand command) in commands.indexed)
              Entrance(
                key: ValueKey<String>('slash-${command.name}'),
                delay: Duration(milliseconds: 35 * index),
                rise: 10,
                scale: 0.98,
                child: InkWell(
                  onTap: () => onPick(command),
                  child: Container(
                    constraints: const BoxConstraints(
                      minHeight: Metric.minTarget,
                    ),
                    padding: const EdgeInsets.symmetric(
                      horizontal: Space.md,
                      vertical: Space.sm,
                    ),
                    child: Row(
                      children: <Widget>[
                        Text(
                          '/${command.name}',
                          style: kit.mono.copyWith(color: kit.accentText),
                        ),
                        const SizedBox(width: Space.md),
                        Expanded(
                          child: Text(
                            command.description,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: kit.meta,
                          ),
                        ),
                      ],
                    ),
                  ),
                ),
              ),
          ],
        ),
      ),
    );
  }
}

class _LockedRow extends StatelessWidget {
  const _LockedRow({required this.text});
  final String text;

  @override
  Widget build(BuildContext context) {
    return Semantics(
      label: text,
      excludeSemantics: true,
      child: Container(
        constraints: const BoxConstraints(minHeight: Metric.inputHeight),
        padding: const EdgeInsets.symmetric(
          horizontal: Space.md,
          vertical: Space.sm,
        ),
        decoration: BoxDecoration(
          color: Kit.of(context).window,
          borderRadius: BorderRadius.circular(Radii.md),
          border: Border.all(color: Kit.of(context).hairline),
        ),
        child: Row(
          children: <Widget>[
            Icon(
              Icons.visibility_outlined,
              size: 18,
              color: Kit.of(context).textMuted,
            ),
            const SizedBox(width: Space.md),
            Expanded(
              child: Text(
                text,
                style: Kit.of(
                  context,
                ).bodyMd.copyWith(color: Kit.of(context).textMuted),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _TakeoverRow extends StatelessWidget {
  const _TakeoverRow({
    required this.info,
    required this.progress,
    required this.onTakeover,
  });
  final TakeoverInfo info;
  final TakeoverProgress progress;
  final VoidCallback? onTakeover;

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    if (!info.available && !info.stoppable) {
      return _LockedRow(text: info.reason ?? l10n.viewOnlyReason);
    }
    final bool running = progress is TakeoverRunning;
    final TakeoverProgress current = progress;
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        if (current is TakeoverFailed)
          Padding(
            padding: const EdgeInsets.only(bottom: Space.sm),
            child: Row(
              children: <Widget>[
                Icon(
                  Icons.error_outline,
                  size: 16,
                  color: Kit.of(context).danger,
                ),
                const SizedBox(width: Space.sm),
                Expanded(
                  child: Text(
                    l10n.takeoverFailed(current.message),
                    style: Kit.of(
                      context,
                    ).bodySm.copyWith(color: Kit.of(context).danger),
                  ),
                ),
              ],
            ),
          ),
        _LockedRow(text: l10n.takeoverHint),
        const SizedBox(height: Space.sm),
        PrimaryButton(
          label: running ? l10n.takeoverRunning : l10n.takeover,
          icon: running ? null : Icons.login,
          onPressed: running ? null : onTakeover,
        ),
      ],
    );
  }
}

class _VoiceNotice extends StatelessWidget {
  const _VoiceNotice({
    required this.voice,
    required this.onOpenSettings,
    required this.onDismiss,
  });
  final VoiceState voice;
  final VoidCallback? onOpenSettings;
  final VoidCallback? onDismiss;

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final VoiceState state = voice;
    final String? text = switch (state) {
      VoiceFailed(problem: VoiceProblem.permissionDenied) =>
        l10n.voicePermissionDenied,
      VoiceFailed(problem: VoiceProblem.noSpeech) => l10n.voiceNoSpeech,
      VoiceFailed(problem: VoiceProblem.engineError) => l10n.voiceEngineError,
      VoiceFailed(problem: VoiceProblem.recorderError) =>
        l10n.voiceRecorderError,
      VoiceIdle(stoppedAtLimit: true) => l10n.voiceTooLong,
      _ => null,
    };
    if (text == null) return const SizedBox.shrink();
    final bool denied =
        state is VoiceFailed && state.problem == VoiceProblem.permissionDenied;
    final bool hint = state is VoiceIdle;
    return Padding(
      padding: const EdgeInsets.only(bottom: Space.sm),
      child: Semantics(
        liveRegion: true,
        child: Row(
          children: <Widget>[
            Icon(
              hint ? Icons.info_outline : Icons.mic_off,
              size: 16,
              color: hint ? Kit.of(context).textMuted : Kit.of(context).danger,
            ),
            const SizedBox(width: Space.sm),
            Expanded(
              child: Text(
                text,
                style: Kit.of(context).bodySm.copyWith(
                  color: hint
                      ? Kit.of(context).textMuted
                      : Kit.of(context).danger,
                ),
              ),
            ),
            if (denied && onOpenSettings != null)
              TextButton(
                onPressed: onOpenSettings,
                child: Text(l10n.voiceOpenSettings),
              ),
            IconButton(
              onPressed: onDismiss,
              icon: const Icon(Icons.close, size: 18),
              tooltip: l10n.close,
            ),
          ],
        ),
      ),
    );
  }
}
