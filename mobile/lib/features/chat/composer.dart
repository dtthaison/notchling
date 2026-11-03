import 'package:flutter/material.dart';

import '../../app/theme/tokens.dart';
import '../../app/widgets/common.dart';
import '../../l10n/app_localizations.dart';
import '../../core/protocol/models.dart';
import '../voice/mic_button.dart';
import '../voice/voice_controller.dart';
import 'chat_notifier.dart';

/// The input box. Enter on the keyboard inserts a newline on a phone; the Send button sends.
/// A view-only session locks it and explains why; a failed send shows the error with Thử lại.
class Composer extends StatelessWidget {
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
  });

  final TextEditingController controller;
  final SendState send;
  final bool viewOnly;

  /// Set instead of the view-only sentence when the screen is a subagent transcript.
  final String? readOnlyReason;
  /// What the Mac says about taking this view-only session over; null when it did not say.
  final TakeoverInfo? takeover;
  final TakeoverProgress takeoverProgress;
  final VoidCallback? onTakeover;

  /// Dictation: the state to show, the mic tap, and the actions of the problem row.
  final VoiceState voice;
  final VoidCallback? onMic;
  final VoidCallback? onOpenSettings;
  final VoidCallback? onDismissVoice;
  final FocusNode? focusNode;
  final VoidCallback onSend;
  final VoidCallback onRetry;
  final VoidCallback onDismissError;

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final SendState state = send;
    final bool sending = state is SendSending;
    return Container(
      decoration: const BoxDecoration(color: Token.surface, border: Border(top: BorderSide(color: Token.border))),
      padding: const EdgeInsets.fromLTRB(Space.md, Space.sm, Space.md, Space.sm),
      child: SafeArea(
        top: false,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: <Widget>[
            if (state is SendFailed && !state.viewOnly)
              Padding(
                padding: const EdgeInsets.only(bottom: Space.sm),
                child: Row(children: <Widget>[
                  const Icon(Icons.error_outline, size: 16, color: Token.statusError),
                  const SizedBox(width: Space.sm),
                  Expanded(child: Text(l10n.sendFailed, style: TypeToken.bodySm.copyWith(color: Token.statusError))),
                  TextButton(onPressed: onRetry, child: Text(l10n.retry)),
                  IconButton(onPressed: onDismissError, icon: const Icon(Icons.close, size: 18), tooltip: l10n.close),
                ]),
              ),
            if (!viewOnly && readOnlyReason == null) _VoiceNotice(voice: voice, onOpenSettings: onOpenSettings, onDismiss: onDismissVoice),
            if (viewOnly && readOnlyReason == null && takeover != null)
              _TakeoverRow(info: takeover!, progress: takeoverProgress, onTakeover: onTakeover)
            else if (viewOnly || readOnlyReason != null)
              _LockedRow(text: readOnlyReason ?? l10n.viewOnlyReason)
            else
              Row(
                crossAxisAlignment: CrossAxisAlignment.end,
                children: <Widget>[
                  MicButton(state: voice, onPressed: onMic),
                  Expanded(
                    child: TextField(
                      controller: controller,
                      focusNode: focusNode,
                      enabled: !sending,
                      minLines: 1,
                      maxLines: 5,
                      keyboardType: TextInputType.multiline,
                      textInputAction: TextInputAction.newline,
                      style: TypeToken.bodyMd,
                      decoration: InputDecoration(hintText: l10n.composerHint),
                    ),
                  ),
                  const SizedBox(width: Space.sm),
                  ValueListenableBuilder<TextEditingValue>(
                    valueListenable: controller,
                    builder: (BuildContext context, TextEditingValue value, Widget? child) => PrimaryButton(
                      label: sending ? l10n.sending : l10n.send,
                      icon: sending ? null : Icons.send,
                      onPressed: sending || value.text.trim().isEmpty ? null : onSend,
                    ),
                  ),
                ],
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
        padding: const EdgeInsets.symmetric(horizontal: Space.md, vertical: Space.sm),
        decoration: BoxDecoration(color: Token.neutral, borderRadius: BorderRadius.circular(Radii.md), border: Border.all(color: Token.border)),
        child: Row(children: <Widget>[
          const Icon(Icons.visibility_outlined, size: 18, color: Token.muted),
          const SizedBox(width: Space.md),
          Expanded(child: Text(text, style: TypeToken.bodyMd.copyWith(color: Token.muted))),
        ]),
      ),
    );
  }
}

/// Replaces the dead-end "view only" sentence on a session the Mac could take over; when it cannot, the
/// Mac's own reason is shown instead.
class _TakeoverRow extends StatelessWidget {
  const _TakeoverRow({required this.info, required this.progress, required this.onTakeover});
  final TakeoverInfo info;
  final TakeoverProgress progress;
  final VoidCallback? onTakeover;

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    if (!info.available && !info.stoppable) return _LockedRow(text: info.reason ?? l10n.viewOnlyReason);
    final bool running = progress is TakeoverRunning;
    final TakeoverProgress current = progress;
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        if (current is TakeoverFailed)
          Padding(
            padding: const EdgeInsets.only(bottom: Space.sm),
            child: Row(children: <Widget>[
              const Icon(Icons.error_outline, size: 16, color: Token.statusError),
              const SizedBox(width: Space.sm),
              Expanded(child: Text(l10n.takeoverFailed(current.message), style: TypeToken.bodySm.copyWith(color: Token.statusError))),
            ]),
          ),
        _LockedRow(text: l10n.takeoverHint),
        const SizedBox(height: Space.sm),
        PrimaryButton(label: running ? l10n.takeoverRunning : l10n.takeover, icon: running ? null : Icons.login, onPressed: running ? null : onTakeover),
      ],
    );
  }
}

/// Why dictation did not give text, with the way out (open Settings for a denied microphone).
class _VoiceNotice extends StatelessWidget {
  const _VoiceNotice({required this.voice, required this.onOpenSettings, required this.onDismiss});
  final VoiceState voice;
  final VoidCallback? onOpenSettings;
  final VoidCallback? onDismiss;

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final VoiceState state = voice;
    final String? text = switch (state) {
      VoiceFailed(problem: VoiceProblem.permissionDenied) => l10n.voicePermissionDenied,
      VoiceFailed(problem: VoiceProblem.noSpeech) => l10n.voiceNoSpeech,
      VoiceFailed(problem: VoiceProblem.engineError) => l10n.voiceEngineError,
      VoiceFailed(problem: VoiceProblem.recorderError) => l10n.voiceRecorderError,
      VoiceIdle(stoppedAtLimit: true) => l10n.voiceTooLong,
      _ => null,
    };
    if (text == null) return const SizedBox.shrink();
    final bool denied = state is VoiceFailed && state.problem == VoiceProblem.permissionDenied;
    final bool hint = state is VoiceIdle;
    return Padding(
      padding: const EdgeInsets.only(bottom: Space.sm),
      child: Semantics(
        liveRegion: true,
        child: Row(children: <Widget>[
          Icon(hint ? Icons.info_outline : Icons.mic_off, size: 16, color: hint ? Token.muted : Token.statusError),
          const SizedBox(width: Space.sm),
          Expanded(child: Text(text, style: TypeToken.bodySm.copyWith(color: hint ? Token.muted : Token.statusError))),
          if (denied && onOpenSettings != null) TextButton(onPressed: onOpenSettings, child: Text(l10n.voiceOpenSettings)),
          IconButton(onPressed: onDismiss, icon: const Icon(Icons.close, size: 18), tooltip: l10n.close),
        ]),
      ),
    );
  }
}
