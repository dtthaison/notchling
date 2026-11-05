import 'dart:async';
import 'dart:io';
import 'dart:typed_data';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:path_provider/path_provider.dart';
import 'package:whisper_ggml/whisper_ggml.dart';

import 'silence_trim.dart';

/// Turns speech into text. The real one is Whisper small via whisper.cpp; tests use a fake.
abstract class SpeechEngine {
  /// [pcm] is 16 kHz mono 16-bit speech already trimmed of silence. [prompt] is Whisper's initial prompt.
  Future<String> transcribe(Int16List pcm, {required String prompt});
}

const String bundledModelAsset = 'assets/models/ggml-small-q5_1.bin';
/// Size of ggml-small-q5_1.bin, to tell a finished copy from a partial one.
const int expectedModelBytes = 190085487;
const String _modelFileName = 'ggml-small-q5_1.bin';

class SpeechEngineException implements Exception {
  const SpeechEngineException(this.message);
  final String message;

  @override
  String toString() => 'SpeechEngineException: $message';
}

/// whisper.cpp (through `whisper_ggml`) on the bundled model. The model is copied out of the app bundle once,
/// because whisper.cpp needs a file path; inference runs in a background isolate and the model stays loaded
/// between dictations.
class WhisperSpeechEngine implements SpeechEngine {
  WhisperSpeechEngine({this.threads = 4});
  final int threads;

  final Whisper _whisper = const Whisper(model: WhisperModel.small);
  Future<String>? _modelPath;
  Timer? _idleRelease;

  /// The model parked in native memory (about 0.5 GB) is freed this long after the last dictation, so a
  /// session that does not use voice does not carry it. Nothing is loaded before the first dictation.
  static const Duration idleRelease = Duration(seconds: 45);

  /// How long copying the model out of the bundle took the first time (null when it was already there).
  Duration? lastProvisionTime;

  Future<String> modelPath() => _modelPath ??= _provision();

  Future<String> _provision() async {
    final Directory dir = Directory('${(await getApplicationSupportDirectory()).path}/voice');
    final File target = File('${dir.path}/$_modelFileName');
    // Already copied by an earlier run: no need to read 181 MiB out of the bundle again.
    if (target.existsSync() && target.lengthSync() == expectedModelBytes) return target.path;
    final ByteData asset;
    try {
      asset = await rootBundle.load(bundledModelAsset);
    } on Object catch (error) {
      throw SpeechEngineException('Thiếu model giọng nói trong app ($error). Chạy mobile/tool/fetch_model.sh rồi build lại.');
    }
    final Stopwatch watch = Stopwatch()..start();
    await dir.create(recursive: true);
    final File partial = File('${target.path}.part');
    await partial.writeAsBytes(asset.buffer.asUint8List(asset.offsetInBytes, asset.lengthInBytes), flush: true);
    await partial.rename(target.path);
    lastProvisionTime = watch.elapsed;
    return target.path;
  }

  @override
  Future<String> transcribe(Int16List pcm, {required String prompt}) async {
    final Directory temp = await getTemporaryDirectory();
    final File wav = File('${temp.path}/dictation.wav');
    await wav.writeAsBytes(encodeWav(pcm), flush: true);
    try {
      return await transcribeFile(wav.path, prompt: prompt);
    } finally {
      if (wav.existsSync()) await wav.delete();
    }
  }

  /// Transcribes an audio file of any format ffmpeg reads. Also used by the debug benchmark.
  Future<String> transcribeFile(String path, {required String prompt}) async {
    _idleRelease?.cancel();
    final String model = await modelPath();
    try {
      final WhisperTranscribeResponse result = await _whisper.transcribe(
        transcribeRequest: TranscribeRequest(
          audio: path,
          language: 'vi',
          threads: threads,
          isNoTimestamps: true,
          initialPrompt: prompt,
          suppressNonSpeechTokens: true,
          keepModelLoaded: true,
        ),
        modelPath: model,
      );
      return result.text;
    } on Object catch (error) {
      debugPrint('WhisperSpeechEngine.transcribeFile $path: $error');
      throw SpeechEngineException('$error');
    } finally {
      _idleRelease = Timer(idleRelease, () => unawaited(_whisper.releaseModel().catchError((Object e) => debugPrint('releaseModel: $e'))));
    }
  }
}
