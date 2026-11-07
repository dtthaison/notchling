import 'dart:math' as math;
import 'dart:typed_data';

import 'package:agent_console/features/voice/insertion.dart';
import 'package:agent_console/features/voice/prompt_builder.dart';
import 'package:agent_console/features/voice/silence_trim.dart';
import 'package:agent_console/features/voice/transcript_cleanup.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';

Int16List _tone(int ms, {double amplitude = 6000, double hz = 220}) =>
    Int16List.fromList(List<int>.generate(voiceSampleRate * ms ~/ 1000, (int i) => (amplitude * math.sin(2 * math.pi * hz * i / voiceSampleRate)).round()));

Int16List _hiss(int ms, {int amplitude = 40}) {
  final math.Random random = math.Random(7);
  return Int16List.fromList(List<int>.generate(voiceSampleRate * ms ~/ 1000, (int _) => random.nextInt(amplitude * 2) - amplitude));
}

Int16List _join(List<Int16List> parts) => Int16List.fromList(<int>[for (final Int16List p in parts) ...p]);

void main() {
  group('prompt builder', () {
    test('starts with the session words, then the fixed glossary', () {
      final String prompt = buildVoicePrompt(project: 'agent_console', branch: 'feat/voice');
      expect(prompt, contains('agent_console, feat/voice'));
      expect(prompt.indexOf('agent_console'), lessThan(prompt.indexOf('commit')));
      for (final String term in <String>['commit', 'pull request', 'flutter', 'docker', 'await']) {
        expect(prompt, contains(term));
      }
    });

    test('takes file names and identifiers from recent messages, newest first, without duplicates', () {
      final List<String> words = technicalWords(<String>[
        'Mở chat_screen.dart rồi sửa composer.dart',
        'hàm insertAtCursor và buildVoicePrompt trong voice_controller',
        'lại insertAtCursor lần nữa',
      ]);
      expect(words.first, 'insertAtCursor');
      expect(words, containsAll(<String>['buildVoicePrompt', 'voice_controller', 'chat_screen.dart', 'composer.dart']));
      expect(words.where((String w) => w == 'insertAtCursor'), hasLength(1));
    });

    test('ignores plain words and very short matches', () {
      expect(technicalWords(<String>['chạy test đi rồi push lên main']), isEmpty);
    });

    test('only the last 20 messages count', () {
      final List<String> texts = <String>['oldIdentifierName here', ...List<String>.generate(20, (int i) => 'plain text $i')];
      expect(buildVoicePrompt(recentTexts: texts), isNot(contains('oldIdentifierName')));
    });

    test('is capped at the prompt length and never cuts a term in half', () {
      final String prompt = buildVoicePrompt(project: 'p', recentTexts: List<String>.generate(40, (int i) => 'someLongIdentifier$i anotherIdentifier$i.dart'));
      expect(prompt.length, lessThanOrEqualTo(maxPromptChars));
      expect(prompt.endsWith('.'), isTrue);
      expect(prompt, isNot(matches(RegExp(r'Identifier\d*,? ?\.$'))), reason: 'ends on a whole term');
    });

    test('the glossary has no duplicates and the prompt lists each term once', () {
      final String prompt = buildVoicePrompt(project: 'Flutter', branch: 'main');
      expect('flutter'.allMatches(prompt.toLowerCase()), hasLength(1));
    });
  });

  group('transcript cleanup', () {
    test('collapses character runs', () {
      expect(cleanTranscript('OVVVVVV'), 'OV');
      expect(cleanTranscript('đúng rồiiiii nhé'), 'đúng rồi nhé');
      expect(cleanTranscript('aaa'), 'aaa', reason: 'three is a word, not a loop');
    });

    test('collapses repeated words and phrases but keeps a doubled word', () {
      expect(cleanTranscript('đúng đúng đúng đúng rồi'), 'đúng rồi');
      expect(cleanTranscript('chạy test chạy test chạy test xong'), 'chạy test xong');
      expect(cleanTranscript('Bug bug rồi'), 'Bug bug rồi');
      expect(cleanTranscript('ok OK ok. Ok'), 'ok');
    });

    test('strips hallucinated boilerplate', () {
      expect(cleanTranscript('Sửa lỗi này đi. Hãy subscribe cho kênh Ghiền Mì Gõ để không bỏ lỡ những video hấp dẫn'), 'Sửa lỗi này đi.');
      expect(cleanTranscript('Cảm ơn các bạn đã xem video!'), '');
      expect(cleanTranscript('Thanks for watching!'), '');
      expect(cleanTranscript('Hãy đăng ký kênh để ủng hộ'), '');
    });

    test('keeps real talk about subscribing and registering', () {
      expect(cleanTranscript('subscribe vào stream rồi đăng ký listener'), 'subscribe vào stream rồi đăng ký listener');
    });

    test('drops CJK and bracket tags, then trims and squeezes spaces', () {
      expect(cleanTranscript('  [Âm nhạc]  chạy   flutter 谢谢 analyze  '), 'chạy flutter analyze');
      expect(cleanTranscript('[BLANK_AUDIO]'), '');
    });
  });

  group('silence trimming', () {
    test('cuts leading and trailing silence but keeps a pad around the speech', () {
      final Int16List pcm = _join(<Int16List>[_hiss(1000), _tone(1000), _hiss(1500)]);
      final Int16List trimmed = trimSilence(pcm);
      final double seconds = trimmed.length / voiceSampleRate;
      expect(seconds, greaterThan(1.0));
      expect(seconds, lessThan(1.7), reason: '1 s of speech plus about 250 ms either side');
      expect(trimmed.length, lessThan(pcm.length));
    });

    test('silence alone is no speech', () {
      expect(trimSilence(_hiss(2000)), isEmpty);
      expect(trimSilence(Int16List(32000)), isEmpty);
    });

    test('a click shorter than the minimum is not speech', () {
      expect(trimSilence(_join(<Int16List>[_hiss(1000), _tone(60), _hiss(1000)])), isEmpty);
    });

    test('too short to hold a frame is empty, not a crash', () {
      expect(trimSilence(Int16List(5)), isEmpty);
      expect(trimSilence(Int16List(0)), isEmpty);
    });

    test('speech that fills the whole clip comes back whole', () {
      final Int16List pcm = _tone(1500);
      expect(trimSilence(pcm).length, pcm.length);
    });
  });

  group('wav and level', () {
    test('the header describes 16 kHz mono 16-bit and the data length', () {
      final Uint8List wav = encodeWav(_tone(500));
      final ByteData view = ByteData.sublistView(wav);
      expect(String.fromCharCodes(wav.sublist(0, 4)), 'RIFF');
      expect(String.fromCharCodes(wav.sublist(8, 12)), 'WAVE');
      expect(view.getUint32(24, Endian.little), 16000);
      expect(view.getUint16(22, Endian.little), 1);
      expect(view.getUint16(34, Endian.little), 16);
      expect(view.getUint32(40, Endian.little), 16000 ~/ 2 * 2);
      expect(wav.length, 44 + 16000 ~/ 2 * 2);
    });

    test('level is 0 for silence and rises with loudness', () {
      Uint8List bytes(Int16List pcm) => pcm.buffer.asUint8List();
      expect(chunkLevel(bytes(Int16List(800))), 0);
      expect(chunkLevel(bytes(_tone(50, amplitude: 1000))), lessThan(chunkLevel(bytes(_tone(50, amplitude: 8000)))));
      expect(chunkLevel(bytes(_tone(50, amplitude: 30000))), lessThanOrEqualTo(1));
    });
  });

  group('insertAtCursor', () {
    TextEditingController field(String text, [int? at, int? to]) => TextEditingController.fromValue(TextEditingValue(
        text: text, selection: at == null ? const TextSelection.collapsed(offset: -1) : TextSelection(baseOffset: at, extentOffset: to ?? at)));

    test('into an empty field', () {
      final TextEditingController c = field('');
      insertAtCursor(c, 'chạy test');
      expect(c.text, 'chạy test');
      expect(c.selection.baseOffset, 9);
    });

    test('appends with a space when there is text and no cursor', () {
      final TextEditingController c = field('sửa bug');
      insertAtCursor(c, 'rồi push');
      expect(c.text, 'sửa bug rồi push');
    });

    test('inserts at the cursor with spaces where it would touch a word, and leaves the cursor after it', () {
      final TextEditingController c = field('sửa  rồi push', 4);
      insertAtCursor(c, 'bug');
      expect(c.text, 'sửa bug rồi push');
      expect(c.selection.baseOffset, c.text.indexOf('bug') + 3);
    });

    test('replaces a selection', () {
      final TextEditingController c = field('chạy XXX đi', 5, 8);
      insertAtCursor(c, 'flutter test');
      expect(c.text, 'chạy flutter test đi');
    });

    test('does not double a space that is already there', () {
      final TextEditingController c = field('xong ', 5);
      insertAtCursor(c, 'rồi');
      expect(c.text, 'xong rồi');
    });

    test('empty or blank text changes nothing', () {
      final TextEditingController c = field('giữ nguyên');
      insertAtCursor(c, '   ');
      expect(c.text, 'giữ nguyên');
    });
  });
}
