import 'dart:async';
import 'dart:typed_data';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'silence_trim.dart';
import 'speech_engine.dart';
import 'transcript_cleanup.dart';
import 'voice_recorder.dart';

/// Longest dictation; recording stops by itself here and what was said so far is transcribed.
const Duration maxDictation = Duration(seconds: 60);

enum VoiceProblem { permissionDenied, noSpeech, engineError, recorderError }

sealed class VoiceState {
  const VoiceState();
}

class VoiceIdle extends VoiceState {
  const VoiceIdle({this.stoppedAtLimit = false});

  /// The last dictation hit the 60 s limit; shown once as a hint.
  final bool stoppedAtLimit;
}

class VoiceRecording extends VoiceState {
  const VoiceRecording({required this.elapsed, required this.level});
  final Duration elapsed;
  final double level;
}

class VoiceTranscribing extends VoiceState {
  const VoiceTranscribing();
}

class VoiceFailed extends VoiceState {
  const VoiceFailed(this.problem, [this.detail]);
  final VoiceProblem problem;
  final String? detail;
}

final Provider<VoiceRecorder> voiceRecorderProvider = Provider<VoiceRecorder>((Ref ref) {
  final RecordPackageRecorder recorder = RecordPackageRecorder();
  ref.onDispose(() => unawaited(recorder.dispose()));
  return recorder;
});

final Provider<SpeechEngine> speechEngineProvider = Provider<SpeechEngine>((Ref ref) => WhisperSpeechEngine());

/// Dictation lifetimes override this in tests to shorten the limit.
final Provider<Duration> maxDictationProvider = Provider<Duration>((Ref ref) => maxDictation);

final NotifierProvider<VoiceController, VoiceState> voiceProvider = NotifierProvider<VoiceController, VoiceState>(VoiceController.new);

/// Tap to start, tap to stop: record, trim silence, transcribe off the UI isolate, clean the text up, and hand
/// it to whoever listens on [transcripts]. Nothing is ever sent from here; the text only lands in the composer.
class VoiceController extends Notifier<VoiceState> {
  final StreamController<String> _transcripts = StreamController<String>.broadcast();
  StreamSubscription<double>? _levelSubscription;
  Timer? _ticker;
  Stopwatch _clock = Stopwatch();
  double _level = 0;
  String _prompt = '';
  int _generation = 0;

  Stream<String> get transcripts => _transcripts.stream;

  @override
  VoiceState build() {
    ref.onDispose(() {
      _ticker?.cancel();
      unawaited(_levelSubscription?.cancel());
      unawaited(_transcripts.close());
    });
    return const VoiceIdle();
  }

  /// Starts recording. [prompt] is Whisper's initial prompt for this dictation.
  Future<void> start({required String prompt}) async {
    if (state is VoiceRecording || state is VoiceTranscribing) return;
    final VoiceRecorder recorder = ref.read(voiceRecorderProvider);
    _prompt = prompt;
    try {
      if (await recorder.ensurePermission() != MicPermission.granted) {
        if (ref.mounted) state = const VoiceFailed(VoiceProblem.permissionDenied);
        return;
      }
      _generation++;
      _level = 0;
      _clock = Stopwatch()..start();
      _levelSubscription = recorder.levels.listen((double value) => _level = value);
      await recorder.start();
    } on Object catch (error) {
      await _stopListening();
      if (ref.mounted) state = VoiceFailed(VoiceProblem.recorderError, '$error');
      return;
    }
    if (!ref.mounted) return;
    state = const VoiceRecording(elapsed: Duration.zero, level: 0);
    final Duration limit = ref.read(maxDictationProvider);
    _ticker = Timer.periodic(const Duration(milliseconds: 100), (Timer timer) {
      if (!ref.mounted) return;
      if (_clock.elapsed >= limit) {
        unawaited(finish(hitLimit: true));
      } else if (state is VoiceRecording) {
        state = VoiceRecording(elapsed: _clock.elapsed, level: _level);
      }
    });
  }

  /// Stops recording and transcribes. Returns the cleaned text, or null when there is none (the state says why).
  Future<String?> finish({bool hitLimit = false}) async {
    if (state is! VoiceRecording) return null;
    final VoiceRecorder recorder = ref.read(voiceRecorderProvider);
    final int generation = _generation;
    await _stopListening();
    state = const VoiceTranscribing();
    try {
      final Int16List recorded = await recorder.stop();
      final Int16List speech = trimSilence(recorded);
      if (speech.isEmpty) return _fail(generation, const VoiceFailed(VoiceProblem.noSpeech));
      final String raw = await ref.read(speechEngineProvider).transcribe(speech, prompt: _prompt);
      if (!ref.mounted || generation != _generation) return null;
      final String text = cleanTranscript(raw);
      if (text.isEmpty) return _fail(generation, const VoiceFailed(VoiceProblem.noSpeech));
      state = VoiceIdle(stoppedAtLimit: hitLimit);
      _transcripts.add(text);
      return text;
    } on SpeechEngineException catch (error) {
      return _fail(generation, VoiceFailed(VoiceProblem.engineError, error.message));
    } on Object catch (error) {
      return _fail(generation, VoiceFailed(VoiceProblem.recorderError, '$error'));
    }
  }

  /// Drops a recording or a transcription in progress. An inference already running in its isolate finishes,
  /// but its text is thrown away.
  Future<void> cancel() async {
    _generation++;
    final bool recording = state is VoiceRecording;
    await _stopListening();
    if (recording) {
      try {
        await ref.read(voiceRecorderProvider).cancel();
      } on Object {
        // Nothing left to cancel.
      }
    }
    if (ref.mounted) state = const VoiceIdle();
  }

  void dismissProblem() {
    if (state is VoiceFailed || (state is VoiceIdle && (state as VoiceIdle).stoppedAtLimit)) state = const VoiceIdle();
  }

  String? _fail(int generation, VoiceFailed failure) {
    if (ref.mounted && generation == _generation) state = failure;
    return null;
  }

  Future<void> _stopListening() async {
    _ticker?.cancel();
    _ticker = null;
    _clock.stop();
    await _levelSubscription?.cancel();
    _levelSubscription = null;
  }
}
