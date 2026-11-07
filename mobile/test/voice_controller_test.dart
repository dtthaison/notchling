import 'dart:async';
import 'dart:math' as math;
import 'dart:typed_data';

import 'package:agent_console/features/chat/chat_notifier.dart';
import 'package:agent_console/features/chat/composer.dart';
import 'package:agent_console/features/voice/mic_button.dart';
import 'package:agent_console/features/voice/speech_engine.dart';
import 'package:agent_console/features/voice/voice_controller.dart';
import 'package:agent_console/features/voice/voice_recorder.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:flutter_test/flutter_test.dart';

import 'support/harness.dart';

Int16List _speech() => Int16List.fromList(<int>[
      ...List<int>.filled(8000, 0),
      ...List<int>.generate(16000, (int i) => (6000 * math.sin(2 * math.pi * 220 * i / 16000)).round()),
      ...List<int>.filled(8000, 0),
    ]);

class FakeRecorder implements VoiceRecorder {
  MicPermission permission = MicPermission.granted;
  Int16List pcm = _speech();
  Object? startError;
  int cancels = 0;
  final StreamController<double> level = StreamController<double>.broadcast();

  @override
  Future<MicPermission> ensurePermission() async => permission;
  @override
  Stream<double> get levels => level.stream;
  @override
  Future<void> start() async {
    if (startError != null) throw startError!;
  }

  @override
  Future<Int16List> stop() async => pcm;
  @override
  Future<void> cancel() async => cancels++;
  @override
  Future<void> dispose() async {}
}

class FakeEngine implements SpeechEngine {
  String text = 'chạy flutter analyze';
  Object? error;
  Completer<String>? gate;
  final List<String> prompts = <String>[];
  int calls = 0;

  @override
  Future<String> transcribe(Int16List pcm, {required String prompt}) async {
    calls++;
    prompts.add(prompt);
    if (error != null) throw error!;
    return gate != null ? gate!.future : text;
  }
}

ProviderContainer _container(FakeRecorder recorder, FakeEngine engine, {Duration? limit}) {
  final ProviderContainer container = ProviderContainer(overrides: <Override>[
    voiceRecorderProvider.overrideWithValue(recorder),
    speechEngineProvider.overrideWithValue(engine),
    if (limit != null) maxDictationProvider.overrideWithValue(limit),
  ]);
  addTearDown(container.dispose);
  container.listen<VoiceState>(voiceProvider, (VoiceState? a, VoiceState b) {});
  return container;
}

void main() {
  group('voice state machine', () {
    test('record, stop, transcribe: the cleaned text is emitted and the state returns to idle', () async {
      final FakeEngine engine = FakeEngine()..text = 'Chạy flutter analyze. Cảm ơn các bạn đã xem video';
      final ProviderContainer c = _container(FakeRecorder(), engine);
      final List<String> heard = <String>[];
      c.read(voiceProvider.notifier).transcripts.listen(heard.add);

      await c.read(voiceProvider.notifier).start(prompt: 'P');
      expect(c.read(voiceProvider), isA<VoiceRecording>());
      await until(() => (c.read(voiceProvider) as VoiceRecording).elapsed > Duration.zero);

      expect(await c.read(voiceProvider.notifier).finish(), 'Chạy flutter analyze.');
      await Future<void>.delayed(Duration.zero);
      expect(heard, <String>['Chạy flutter analyze.']);
      expect(c.read(voiceProvider), isA<VoiceIdle>());
      expect(engine.prompts, <String>['P']);
    });

    test('the level reaches the recording state', () async {
      final FakeRecorder recorder = FakeRecorder();
      final ProviderContainer c = _container(recorder, FakeEngine());
      await c.read(voiceProvider.notifier).start(prompt: '');
      recorder.level.add(0.7);
      await until(() => c.read(voiceProvider) is VoiceRecording && (c.read(voiceProvider) as VoiceRecording).level == 0.7);
    });

    test('permission denied: no recording starts and the state says so', () async {
      final FakeRecorder recorder = FakeRecorder()..permission = MicPermission.denied;
      final ProviderContainer c = _container(recorder, FakeEngine());
      await c.read(voiceProvider.notifier).start(prompt: '');
      final VoiceState state = c.read(voiceProvider);
      expect(state, isA<VoiceFailed>());
      expect((state as VoiceFailed).problem, VoiceProblem.permissionDenied);
      c.read(voiceProvider.notifier).dismissProblem();
      expect(c.read(voiceProvider), isA<VoiceIdle>());
    });

    test('a recorder that cannot start is a recorder error', () async {
      final ProviderContainer c = _container(FakeRecorder()..startError = StateError('busy'), FakeEngine());
      await c.read(voiceProvider.notifier).start(prompt: '');
      expect((c.read(voiceProvider) as VoiceFailed).problem, VoiceProblem.recorderError);
    });

    test('silence is "no speech" and the engine is never asked', () async {
      final FakeEngine engine = FakeEngine();
      final ProviderContainer c = _container(FakeRecorder()..pcm = Int16List(32000), engine);
      await c.read(voiceProvider.notifier).start(prompt: '');
      expect(await c.read(voiceProvider.notifier).finish(), isNull);
      expect((c.read(voiceProvider) as VoiceFailed).problem, VoiceProblem.noSpeech);
      expect(engine.calls, 0);
    });

    test('text that is all boilerplate is "no speech" too', () async {
      final ProviderContainer c = _container(FakeRecorder(), FakeEngine()..text = 'Hãy subscribe cho kênh Ghiền Mì Gõ');
      await c.read(voiceProvider.notifier).start(prompt: '');
      expect(await c.read(voiceProvider.notifier).finish(), isNull);
      expect((c.read(voiceProvider) as VoiceFailed).problem, VoiceProblem.noSpeech);
    });

    test('an engine error keeps its message and emits nothing', () async {
      final List<String> heard = <String>[];
      final ProviderContainer c = _container(FakeRecorder(), FakeEngine()..error = const SpeechEngineException('model missing'));
      c.read(voiceProvider.notifier).transcripts.listen(heard.add);
      await c.read(voiceProvider.notifier).start(prompt: '');
      expect(await c.read(voiceProvider.notifier).finish(), isNull);
      final VoiceFailed failed = c.read(voiceProvider) as VoiceFailed;
      expect(failed.problem, VoiceProblem.engineError);
      expect(failed.detail, 'model missing');
      expect(heard, isEmpty);
    });

    test('cancel while recording drops the audio', () async {
      final FakeRecorder recorder = FakeRecorder();
      final FakeEngine engine = FakeEngine();
      final ProviderContainer c = _container(recorder, engine);
      await c.read(voiceProvider.notifier).start(prompt: '');
      await c.read(voiceProvider.notifier).cancel();
      expect(c.read(voiceProvider), isA<VoiceIdle>());
      expect(recorder.cancels, 1);
      expect(engine.calls, 0);
    });

    test('cancel while transcribing throws the text away', () async {
      final FakeEngine engine = FakeEngine()..gate = Completer<String>();
      final ProviderContainer c = _container(FakeRecorder(), engine);
      final List<String> heard = <String>[];
      c.read(voiceProvider.notifier).transcripts.listen(heard.add);
      await c.read(voiceProvider.notifier).start(prompt: '');
      final Future<String?> finishing = c.read(voiceProvider.notifier).finish();
      await until(() => c.read(voiceProvider) is VoiceTranscribing && engine.calls == 1);

      await c.read(voiceProvider.notifier).cancel();
      expect(c.read(voiceProvider), isA<VoiceIdle>());
      engine.gate!.complete('lời đến muộn');
      expect(await finishing, isNull);
      await Future<void>.delayed(const Duration(milliseconds: 20));
      expect(heard, isEmpty);
      expect(c.read(voiceProvider), isA<VoiceIdle>());
    });

    test('the limit stops the recording by itself, transcribes what was said, and leaves a hint', () async {
      final FakeEngine engine = FakeEngine();
      final ProviderContainer c = _container(FakeRecorder(), engine, limit: const Duration(milliseconds: 250));
      final List<String> heard = <String>[];
      c.read(voiceProvider.notifier).transcripts.listen(heard.add);
      await c.read(voiceProvider.notifier).start(prompt: '');
      await until(() => heard.isNotEmpty);
      final VoiceState state = c.read(voiceProvider);
      expect(state, isA<VoiceIdle>());
      expect((state as VoiceIdle).stoppedAtLimit, isTrue);
      c.read(voiceProvider.notifier).dismissProblem();
      expect((c.read(voiceProvider) as VoiceIdle).stoppedAtLimit, isFalse);
    });

    test('the default limit is 60 seconds', () {
      expect(maxDictation, const Duration(seconds: 60));
    });

    test('a second tap while busy does nothing', () async {
      final FakeRecorder recorder = FakeRecorder();
      final ProviderContainer c = _container(recorder, FakeEngine());
      await c.read(voiceProvider.notifier).start(prompt: 'one');
      await c.read(voiceProvider.notifier).start(prompt: 'two');
      expect(c.read(voiceProvider), isA<VoiceRecording>());
    });
  });

  group('mic button and composer', () {
    Future<void> pump(WidgetTester tester, VoiceState state, {VoidCallback? onMic, VoidCallback? onSettings}) async {
      final TextEditingController controller = TextEditingController();
      addTearDown(controller.dispose);
      await tester.pumpWidget(host(Composer(
        controller: controller,
        send: const SendIdle(),
        viewOnly: false,
        onSend: () {},
        onRetry: () {},
        onDismissError: () {},
        voice: state,
        onMic: onMic,
        onOpenSettings: onSettings,
        onDismissVoice: () {},
      )));
    }

    testWidgets('idle: tapping the mic calls back and the label says how to start', (WidgetTester tester) async {
      int taps = 0;
      await pump(tester, const VoiceIdle(), onMic: () => taps++);
      expect(find.byTooltip('Nói để nhập'), findsOneWidget);
      await tester.tap(find.byType(MicButton));
      expect(taps, 1);
    });

    testWidgets('recording: shows the timer and the stop label, and still takes a tap', (WidgetTester tester) async {
      int taps = 0;
      await pump(tester, const VoiceRecording(elapsed: Duration(seconds: 12), level: 0.5), onMic: () => taps++);
      expect(find.text('0:12'), findsOneWidget);
      expect(find.byTooltip('Dừng và nhận dạng'), findsOneWidget);
      await tester.tap(find.byType(MicButton));
      expect(taps, 1);
    });

    testWidgets('transcribing: says Đang nhận dạng… and a tap cancels', (WidgetTester tester) async {
      int taps = 0;
      await pump(tester, const VoiceTranscribing(), onMic: () => taps++);
      expect(find.text('Đang nhận dạng…'), findsOneWidget);
      expect(find.byTooltip('Huỷ nhận dạng'), findsOneWidget);
      await tester.tap(find.byType(MicButton));
      expect(taps, 1);
    });

    testWidgets('reduce motion keeps the halo a steady size', (WidgetTester tester) async {
      final TextEditingController controller = TextEditingController();
      addTearDown(controller.dispose);
      Future<double> haloWidth(double level) async {
        await tester.pumpWidget(MediaQuery(
          data: const MediaQueryData(disableAnimations: true),
          child: host(MicButton(state: VoiceRecording(elapsed: Duration.zero, level: level), onPressed: () {})),
        ));
        await tester.pump();
        return tester.getSize(find.byType(AnimatedContainer)).width;
      }

      expect(await haloWidth(0.1), await haloWidth(0.9));
    });

    testWidgets('permission denied: the message offers Mở cài đặt', (WidgetTester tester) async {
      int opened = 0;
      await pump(tester, const VoiceFailed(VoiceProblem.permissionDenied), onMic: () {}, onSettings: () => opened++);
      expect(find.text('Chưa cho phép dùng micro.'), findsOneWidget);
      await tester.tap(find.text('Mở cài đặt'));
      expect(opened, 1);
    });

    testWidgets('no speech, engine error and the 60 s hint each show their own text', (WidgetTester tester) async {
      await pump(tester, const VoiceFailed(VoiceProblem.noSpeech));
      expect(find.textContaining('Không nghe thấy'), findsOneWidget);
      await pump(tester, const VoiceFailed(VoiceProblem.engineError));
      expect(find.textContaining('Nhận dạng lỗi'), findsOneWidget);
      await pump(tester, const VoiceIdle(stoppedAtLimit: true));
      expect(find.text('Đã dừng ở 60 giây.'), findsOneWidget);
      expect(find.text('Mở cài đặt'), findsNothing);
    });
  });
}
