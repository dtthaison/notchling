import 'dart:async';
import 'dart:io';
import 'dart:typed_data';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:path_provider/path_provider.dart';
import 'package:whisper_ggml/whisper_ggml.dart';

import 'silence_trim.dart';

abstract class SpeechEngine {
  Future<String> transcribe(Int16List pcm, {required String prompt});
}

const String bundledModelAsset = 'assets/models/ggml-small-q5_1.bin';
const int expectedModelBytes = 190085487;
const String _modelFileName = 'ggml-small-q5_1.bin';

class SpeechEngineException implements Exception {
  const SpeechEngineException(this.message, {this.modelMissing = false});
  final String message;
  final bool modelMissing;

  @override
  String toString() => 'SpeechEngineException: $message';
}

class WhisperSpeechEngine implements SpeechEngine {
  WhisperSpeechEngine({this.threads = 4});
  final int threads;

  final Whisper _whisper = const Whisper(model: WhisperModel.small);
  Future<String>? _modelPath;
  Timer? _idleRelease;

  static const Duration idleRelease = Duration(seconds: 45);

  Duration? lastProvisionTime;

  Future<String> modelPath() => _modelPath ??= _provision();

  Future<String> _provision() async {
    final Directory dir = Directory('${(await getApplicationSupportDirectory()).path}/voice');
    final File target = File('${dir.path}/$_modelFileName');
    if (target.existsSync() && target.lengthSync() == expectedModelBytes) return target.path;
    final ByteData asset;
    try {
      asset = await rootBundle.load(bundledModelAsset);
    } on Object catch (error) {
      throw SpeechEngineException('The speech model is missing from the app ($error). Run apps/flutter/tool/fetch_model.sh and build again.', modelMissing: true);
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
