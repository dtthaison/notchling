import 'dart:async';
import 'dart:math' as math;
import 'dart:typed_data';

import 'package:agent_console/features/chat/slash_commands.dart';
import 'package:agent_console/features/visualize/viz_providers.dart';
import 'package:agent_console/features/voice/speech_engine.dart';
import 'package:agent_console/features/voice/voice_controller.dart';
import 'package:agent_console/features/voice/voice_recorder.dart';
import 'package:agent_console/l10n/app_localizations_en.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:flutter_test/flutter_test.dart';

class _Recorder implements VoiceRecorder {
  @override
  Future<MicPermission> ensurePermission() async => MicPermission.granted;
  @override
  Stream<double> get levels => const Stream<double>.empty();
  @override
  Future<void> start() async {}
  @override
  Future<Int16List> stop() async => Int16List.fromList(<int>[
        ...List<int>.filled(8000, 0),
        ...List<int>.generate(16000, (int i) => (6000 * math.sin(2 * math.pi * 220 * i / 16000)).round()),
        ...List<int>.filled(8000, 0),
      ]);
  @override
  Future<void> cancel() async {}
  @override
  Future<void> dispose() async {}
}

class _Failing implements SpeechEngine {
  _Failing(this.error);
  final Object error;

  @override
  Future<String> transcribe(Int16List pcm, {required String prompt}) async => throw error;
}

Future<VoiceFailed> _failWith(Object error) async {
  final ProviderContainer container = ProviderContainer(overrides: <Override>[
    voiceRecorderProvider.overrideWithValue(_Recorder()),
    speechEngineProvider.overrideWithValue(_Failing(error)),
  ]);
  addTearDown(container.dispose);
  container.listen<VoiceState>(voiceProvider, (VoiceState? a, VoiceState b) {});
  await container.read(voiceProvider.notifier).start(prompt: '');
  await container.read(voiceProvider.notifier).finish();
  return container.read(voiceProvider) as VoiceFailed;
}

void main() {
  final AppL10nEn l10n = AppL10nEn();

  test('built_in_slash_commands_come_from_messages', () {
    expect(builtInSlashCommands, hasLength(16));
    expect(builtInSlashCommands.first.name, 'clear');
    expect(builtInSlashCommands.first.description, l10n.chatSlashClear);
    expect(builtInSlashCommands.map((SlashCommand c) => c.description), everyElement(isNotEmpty));
    expect(builtInSlashCommandsFor(l10n).map((SlashCommand c) => c.description), builtInSlashCommands.map((SlashCommand c) => c.description));
  });

  test('visualize_tabs_have_english_titles', () {
    expect(<String>[for (final VizTab tab in VizTab.values) tab.title(l10n)], <String>['Phone', 'Web', 'Flow', 'Diagram', 'Entity', 'Chart']);
  });

  test('a_missing_speech_model_has_its_own_problem', () async {
    final VoiceFailed failed = await _failWith(const SpeechEngineException('model file absent', modelMissing: true));
    expect(failed.problem, VoiceProblem.modelMissing);
    expect(l10n.voiceModelMissing, 'The speech model is missing from this app.');
  });

  test('any_other_engine_error_stays_an_engine_error', () async {
    final VoiceFailed failed = await _failWith(const SpeechEngineException('boom'));
    expect(failed.problem, VoiceProblem.engineError);
  });

  test('accessibility_labels_count_sessions', () {
    expect(l10n.sessionsGroupLabel('Working', 2), 'Working, 2 session');
    expect(l10n.macsComputerHeadingLabel('Studio', 'Connected', 1), 'Studio, Connected, 1 session');
  });
}
