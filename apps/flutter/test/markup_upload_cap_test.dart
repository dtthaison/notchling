import 'dart:async';
import 'dart:math' as math;
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:agent_console/features/chat/markup_editor.dart';
import 'package:flutter_test/flutter_test.dart';

Future<ui.Image> noisy(int width, int height) {
  final math.Random random = math.Random(7);
  final Uint8List pixels = Uint8List(width * height * 4);
  for (int i = 0; i < pixels.length; i++) {
    pixels[i] = i % 4 == 3 ? 255 : random.nextInt(256);
  }
  final Completer<ui.Image> done = Completer<ui.Image>();
  ui.decodeImageFromPixels(pixels, width, height, ui.PixelFormat.rgba8888, done.complete);
  return done.future;
}

void main() {
  test('the upload cap is the Mac cap', () {
    expect(uploadCapBytes, 6 * 1024 * 1024);
  });

  testWidgets('a marked-up picture too big for the cap is downscaled until it fits, keeping its shape', (WidgetTester tester) async {
    await tester.runAsync(() async {
      final ui.Image source = await noisy(400, 300);
      final Mark ring = Mark(MarkTool.ellipse, markColors.first, const Offset(20, 20))..extend(const Offset(300, 200));
      final Uint8List whole = await renderMarkup(source, <Mark>[ring], maxBytes: 1 << 30);
      expect(whole.length, greaterThan(200 * 1024));

      final Uint8List out = await renderMarkup(source, <Mark>[ring], maxBytes: 200 * 1024);
      expect(out.length, lessThanOrEqualTo(200 * 1024));
      expect(out.sublist(0, 4), <int>[0x89, 0x50, 0x4E, 0x47]);
      final ui.Image back = (await (await ui.instantiateImageCodec(out)).getNextFrame()).image;
      expect(back.width, lessThan(400));
      expect(back.width / back.height, closeTo(4 / 3, 0.02));
    });
  });

  testWidgets('a picture that fits keeps its native size', (WidgetTester tester) async {
    await tester.runAsync(() async {
      final ui.Image source = await noisy(40, 30);
      final Uint8List out = await renderMarkup(source, const <Mark>[]);
      final ui.Image back = (await (await ui.instantiateImageCodec(out)).getNextFrame()).image;
      expect(back.width, 40);
      expect(back.height, 30);
    });
  });

  testWidgets('a picture that cannot be made to fit is refused, not sent over the cap', (WidgetTester tester) async {
    await tester.runAsync(() async {
      final ui.Image source = await noisy(64, 64);
      await expectLater(renderMarkup(source, const <Mark>[], maxBytes: 16), throwsA(isA<MarkupTooBig>()));
    });
  });
}
