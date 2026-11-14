import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:agent_console/app/theme/tokens.dart';
import 'package:agent_console/features/chat/markup_editor.dart';
import 'package:agent_console/l10n/app_localizations.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

Future<Uint8List> _png(int width, int height) async {
  final ui.PictureRecorder recorder = ui.PictureRecorder();
  Canvas(recorder).drawRect(Rect.fromLTWH(0, 0, width.toDouble(), height.toDouble()), Paint()..color = const Color(0xFF336699));
  final ui.Image image = await recorder.endRecording().toImage(width, height);
  final ByteData? data = await image.toByteData(format: ui.ImageByteFormat.png);
  image.dispose();
  return data!.buffer.asUint8List();
}

void main() {
  testWidgets("rendering returns a PNG at the picture's native size, with the marks drawn on it", (WidgetTester tester) async {
    await tester.runAsync(() async {
      final ui.Codec codec = await ui.instantiateImageCodec(await _png(120, 80));
      final ui.Image source = (await codec.getNextFrame()).image;
      final Mark ring = Mark(MarkTool.ellipse, markColors.first, const Offset(20, 20))..extend(const Offset(100, 60));
      final Uint8List out = await renderMarkup(source, <Mark>[ring]);
      expect(out.sublist(0, 4), <int>[0x89, 0x50, 0x4E, 0x47]);
      final ui.Image back = (await (await ui.instantiateImageCodec(out)).getNextFrame()).image;
      expect(back.width, 120);
      expect(back.height, 80);
      // The ellipse's left edge is red where the source was blue.
      final ByteData pixels = (await back.toByteData())!;
      final int at = (40 * 120 + 20) * 4;
      expect(pixels.getUint8(at), greaterThan(200));
      expect(pixels.getUint8(at + 2), lessThan(100));
    });
  });

  test('undo drops the last mark, clear drops all', () {
    final MarkupBoard board = MarkupBoard()
      ..add(Mark(MarkTool.pen, markColors.first, Offset.zero))
      ..add(Mark(MarkTool.arrow, markColors.first, Offset.zero));
    board.removeLast();
    expect(board.marks.map((Mark m) => m.tool), <MarkTool>[MarkTool.pen]);
    board.clear();
    expect(board.marks, isEmpty);
  });

  test("strokes scale with the picture's longer side", () {
    expect(markStrokeWidth(const Size(1000, 2000)), closeTo(12, 1e-9));
    expect(markStrokeWidth(const Size(100, 100)), 2);
  });

  testWidgets('a finger draws a mark in picture pixels and Undo takes it away; two fingers draw nothing', (WidgetTester tester) async {
    final MarkupBoard board = MarkupBoard();
    late Uint8List bytes;
    await tester.runAsync(() async => bytes = await _png(200, 100));
    await tester.pumpWidget(ProviderScope(
      child: MaterialApp(
        theme: ThemeData(extensions: <ThemeExtension<dynamic>>[Kit.dark]),
        localizationsDelegates: AppL10n.localizationsDelegates,
        supportedLocales: AppL10n.supportedLocales,
        locale: const Locale('vi'),
        home: MarkupEditor(sessionId: 'c', bytes: bytes, board: board),
      ),
    ));
    await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 200)));
    await tester.pump();
    final Finder canvas = find.byKey(const ValueKey<String>('markup-canvas'));
    await tester.drag(canvas, const Offset(60, 20));
    await tester.pump();
    expect(board.marks, hasLength(1));
    final Mark mark = board.marks.single;
    expect(mark.points.length, greaterThan(1));
    expect(mark.end.dx, inInclusiveRange(0, 200));
    await tester.tap(find.byKey(const ValueKey<String>('markup-undo')));
    await tester.pump();
    expect(board.marks, isEmpty);

    final Offset center = tester.getCenter(canvas);
    final TestGesture a = await tester.startGesture(center - const Offset(20, 0), pointer: 1);
    final TestGesture b = await tester.startGesture(center + const Offset(20, 0), pointer: 2);
    await a.moveTo(center - const Offset(50, 0));
    await b.moveTo(center + const Offset(50, 0));
    await tester.pump();
    await a.up();
    await b.up();
    await tester.pump();
    expect(board.marks, isEmpty, reason: 'a pinch must not leave a mark');
  });
}
