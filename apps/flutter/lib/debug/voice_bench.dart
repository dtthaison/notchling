import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:path_provider/path_provider.dart';

import '../features/voice/prompt_builder.dart';
import '../features/voice/speech_engine.dart';

Future<void> runVoiceBenchIfRequested() async {
  if (!kDebugMode && !const bool.fromEnvironment('VOICE_BENCH')) return;
  final Directory? external = await getExternalStorageDirectory();
  final List<Directory> candidates = <Directory>[Directory('${Directory.systemTemp.path}/voice_bench'), if (external != null) Directory('${external.path}/voice_bench')];
  final Directory? dir = candidates.where((Directory d) => d.existsSync()).firstOrNull;
  if (dir == null) return;
  final String resultsPath = '${dir.parent.path}/voice_bench_results.txt';
  final List<File> files = dir.listSync().whereType<File>().where((File f) => f.path.endsWith('.wav') && !f.path.endsWith('.wav.wav')).toList()
    ..sort((File a, File b) => a.path.compareTo(b.path));
  if (files.isEmpty) return;

  final StringBuffer out = StringBuffer();
  void log(String line) {
    out.writeln(line);
    debugPrint('VOICEBENCH $line');
  }

  final WhisperSpeechEngine engine = WhisperSpeechEngine();
  final String prompt = buildVoicePrompt(project: 'agent_console', branch: 'main');
  log('rss_start_mb=${_rssMb()}');
  final Stopwatch provision = Stopwatch()..start();
  await engine.modelPath();
  log('model_ready_s=${(provision.elapsedMilliseconds / 1000).toStringAsFixed(2)} copy_s=${engine.lastProvisionTime?.inMilliseconds == null ? 'already-there' : (engine.lastProvisionTime!.inMilliseconds / 1000).toStringAsFixed(2)}');

  bool first = true;
  double totalAudio = 0;
  double totalWork = 0;
  for (final File file in files) {
    final double seconds = _wavSeconds(file);
    final Stopwatch watch = Stopwatch()..start();
    String text;
    try {
      text = await engine.transcribeFile(file.path, prompt: prompt);
    } on Object catch (error) {
      text = 'ERROR $error';
    }
    final double work = watch.elapsedMilliseconds / 1000;
    log('${first ? 'COLD ' : ''}${file.uri.pathSegments.last} audio_s=${seconds.toStringAsFixed(2)} work_s=${work.toStringAsFixed(2)} rtf=${(work / seconds).toStringAsFixed(2)} rss_mb=${_rssMb()}');
    log('  text: $text');
    if (!first) {
      totalAudio += seconds;
      totalWork += work;
    }
    first = false;
  }
  if (totalAudio > 0) log('WARM rtf=${(totalWork / totalAudio).toStringAsFixed(2)} over ${totalAudio.toStringAsFixed(1)} s of audio');
  File(resultsPath).writeAsStringSync(out.toString());
}

double _wavSeconds(File file) {
  final Uint8List bytes = file.readAsBytesSync();
  final ByteData view = ByteData.sublistView(bytes);
  int byteRate = 32000;
  int offset = 12;
  while (offset + 8 <= bytes.length) {
    final String id = String.fromCharCodes(bytes.sublist(offset, offset + 4));
    final int size = view.getUint32(offset + 4, Endian.little);
    if (id == 'fmt ') byteRate = view.getUint32(offset + 16, Endian.little);
    if (id == 'data') return size / byteRate;
    offset += 8 + size + (size.isOdd ? 1 : 0);
  }
  return bytes.length / byteRate;
}

String _rssMb() {
  try {
    final String status = File('/proc/self/status').readAsStringSync();
    final RegExpMatch? match = RegExp(r'VmRSS:\s+(\d+) kB').firstMatch(status);
    return match == null ? '?' : (int.parse(match.group(1)!) / 1024).toStringAsFixed(0);
  } on Object {
    return '?';
  }
}
