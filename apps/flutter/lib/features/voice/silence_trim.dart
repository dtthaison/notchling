import 'dart:math' as math;
import 'dart:typed_data';

const int voiceSampleRate = 16000;

/// Cuts leading and trailing silence off 16-bit mono PCM with a simple energy gate: a frame counts as speech
/// when its RMS is well above the quietest tenth of the recording. Returns an empty list when nothing in it
/// sounds like speech, so the caller can say "no speech" instead of feeding Whisper silence (it invents text).
Int16List trimSilence(Int16List pcm, {int sampleRate = voiceSampleRate, int frameMs = 20, int padMs = 250, int minSpeechMs = 200}) {
  final int frame = sampleRate * frameMs ~/ 1000;
  if (frame == 0 || pcm.length < frame) return Int16List(0);
  final int frames = pcm.length ~/ frame;
  final List<double> energy = List<double>.generate(frames, (int f) {
    double sum = 0;
    for (int i = f * frame; i < (f + 1) * frame; i++) {
      sum += pcm[i] * pcm[i].toDouble();
    }
    return math.sqrt(sum / frame);
  });
  final List<double> sorted = List<double>.of(energy)..sort();
  final double floor = sorted[(sorted.length * 0.1).floor().clamp(0, sorted.length - 1)];
  // Capped by the peak so a clip that is speech from start to end (no quiet frames to measure) still counts.
  final double threshold = math.max(200, math.min(floor * 3, sorted.last * 0.35));

  int first = -1;
  int last = -1;
  int voiced = 0;
  for (int f = 0; f < frames; f++) {
    if (energy[f] >= threshold) {
      first = first < 0 ? f : first;
      last = f;
      voiced++;
    }
  }
  if (first < 0 || voiced * frameMs < minSpeechMs) return Int16List(0);
  final int pad = padMs ~/ frameMs;
  final int from = math.max(0, first - pad) * frame;
  final int to = math.min(frames, last + 1 + pad) * frame;
  return Int16List.sublistView(pcm, from, to);
}

/// A 44-byte RIFF header plus the samples: what whisper.cpp reads.
Uint8List encodeWav(Int16List pcm, {int sampleRate = voiceSampleRate}) {
  final int dataBytes = pcm.length * 2;
  final ByteData header = ByteData(44);
  void ascii(int offset, String text) {
    for (int i = 0; i < text.length; i++) {
      header.setUint8(offset + i, text.codeUnitAt(i));
    }
  }

  ascii(0, 'RIFF');
  header.setUint32(4, 36 + dataBytes, Endian.little);
  ascii(8, 'WAVE');
  ascii(12, 'fmt ');
  header.setUint32(16, 16, Endian.little);
  header.setUint16(20, 1, Endian.little);
  header.setUint16(22, 1, Endian.little);
  header.setUint32(24, sampleRate, Endian.little);
  header.setUint32(28, sampleRate * 2, Endian.little);
  header.setUint16(32, 2, Endian.little);
  header.setUint16(34, 16, Endian.little);
  ascii(36, 'data');
  header.setUint32(40, dataBytes, Endian.little);
  final BytesBuilder out = BytesBuilder(copy: false)
    ..add(header.buffer.asUint8List())
    ..add(pcm.buffer.asUint8List(pcm.offsetInBytes, dataBytes));
  return out.toBytes();
}

/// RMS of a PCM16 little-endian chunk as 0..1, for the level meter.
double chunkLevel(Uint8List bytes) {
  final int samples = bytes.length ~/ 2;
  if (samples == 0) return 0;
  final ByteData view = ByteData.sublistView(bytes);
  double sum = 0;
  for (int i = 0; i < samples; i++) {
    final int s = view.getInt16(i * 2, Endian.little);
    sum += s * s.toDouble();
  }
  // Speech sits around 1000-8000 of 32768; this maps it onto most of the meter.
  return (math.sqrt(sum / samples) / 6000).clamp(0.0, 1.0);
}
