import 'dart:math' as math;

import 'package:flutter/material.dart';

import '../../app/theme/tokens.dart';
import '../../app/widgets/common.dart';
import '../../l10n/app_localizations.dart';
import 'voice_controller.dart';

/// The composer's microphone: tap to start, tap to stop. While recording the button breathes with the voice
/// (a halo that follows the level) and a timer runs; while transcribing it says so, and a tap cancels.
/// With Reduce Motion on the halo is a steady ring.
class MicButton extends StatelessWidget {
  const MicButton({super.key, required this.state, required this.onPressed});
  final VoiceState state;

  /// Null disables the button (no connection, say).
  final VoidCallback? onPressed;

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final VoiceState current = state;
    final bool recording = current is VoiceRecording;
    final bool transcribing = current is VoiceTranscribing;
    final String label = recording ? l10n.micStop : (transcribing ? l10n.micCancel : l10n.micStart);
    final bool still = reduceMotion(context);
    final double level = recording ? current.level : 0;

    return Semantics(
      button: true,
      enabled: onPressed != null,
      label: label,
      value: recording ? _clock(current.elapsed) : null,
      excludeSemantics: true,
      onTap: onPressed,
      child: Tooltip(
        message: label,
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: onPressed,
          child: ConstrainedBox(
            constraints: const BoxConstraints(minWidth: Metric.minTarget, minHeight: Metric.minTarget),
            child: Row(mainAxisSize: MainAxisSize.min, children: <Widget>[
              SizedBox(
                width: Metric.minTarget,
                height: Metric.minTarget,
                child: Stack(alignment: Alignment.center, children: <Widget>[
                  if (recording)
                    AnimatedContainer(
                      duration: still ? Duration.zero : const Duration(milliseconds: 120),
                      curve: Curves.easeOut,
                      width: still ? 40 : 28 + 20 * math.sqrt(level),
                      height: still ? 40 : 28 + 20 * math.sqrt(level),
                      decoration: BoxDecoration(shape: BoxShape.circle, color: Kit.of(context).accent.withValues(alpha: still ? 0.18 : 0.12 + 0.2 * level)),
                    ),
                  if (transcribing)
                    SizedBox(width: 22, height: 22, child: CircularProgressIndicator(strokeWidth: 2, color: Kit.of(context).accent))
                  else
                    Icon(recording ? Icons.stop_rounded : Icons.mic_none, size: 22, color: onPressed == null ? Kit.of(context).textFaint : (recording ? Kit.of(context).accent : Kit.of(context).textMuted)),
                ]),
              ),
              if (recording) Text(_clock(current.elapsed), style: Kit.of(context).codeSm.copyWith(color: Kit.of(context).accent)),
              if (transcribing)
                Padding(
                  padding: const EdgeInsets.only(right: Space.sm),
                  child: Text(l10n.transcribing, style: Kit.of(context).bodySm.copyWith(color: Kit.of(context).textMuted)),
                ),
            ]),
          ),
        ),
      ),
    );
  }

  static String _clock(Duration d) => '${d.inMinutes}:${(d.inSeconds % 60).toString().padLeft(2, '0')}';
}
