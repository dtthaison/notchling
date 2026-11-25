import 'dart:async';
import 'dart:typed_data';

import 'package:record/record.dart';

import 'silence_trim.dart';

enum MicPermission { granted, denied }

abstract class VoiceRecorder {
  Future<MicPermission> ensurePermission();

  Stream<double> get levels;

  Future<void> start();

  Future<Int16List> stop();

  Future<void> cancel();

  Future<void> dispose();
}

class RecordPackageRecorder implements VoiceRecorder {
  final AudioRecorder _recorder = AudioRecorder();
  final StreamController<double> _levels = StreamController<double>.broadcast();
  final BytesBuilder _bytes = BytesBuilder(copy: false);
  StreamSubscription<Uint8List>? _subscription;

  @override
  Stream<double> get levels => _levels.stream;

  @override
  Future<MicPermission> ensurePermission() async => await _recorder.hasPermission() ? MicPermission.granted : MicPermission.denied;

  @override
  Future<void> start() async {
    _bytes.clear();
    final Stream<Uint8List> stream = await _recorder.startStream(const RecordConfig(
      encoder: AudioEncoder.pcm16bits,
      sampleRate: voiceSampleRate,
      numChannels: 1,
      echoCancel: false,
      noiseSuppress: false,
    ));
    _subscription = stream.listen((Uint8List chunk) {
      _bytes.add(chunk);
      if (!_levels.isClosed) _levels.add(chunkLevel(chunk));
    });
  }

  @override
  Future<Int16List> stop() async {
    await _recorder.stop();
    await _subscription?.cancel();
    _subscription = null;
    final Uint8List all = _bytes.takeBytes();
    final int samples = all.length ~/ 2;
    return Int16List.view(Uint8List.fromList(all.sublist(0, samples * 2)).buffer);
  }

  @override
  Future<void> cancel() async {
    await _subscription?.cancel();
    _subscription = null;
    _bytes.clear();
    await _recorder.cancel();
  }

  @override
  Future<void> dispose() async {
    await _subscription?.cancel();
    await _levels.close();
    await _recorder.dispose();
  }
}
