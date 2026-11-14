import 'dart:ui';

import 'package:agent_console/features/chat/gallery_math.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  const Size viewport = Size(400, 800);
  const Size content = Size(400, 300); // a 4:3 picture fitted by width

  test('a picture is fitted inside the viewport, whichever side is the tight one', () {
    expect(fittedSize(viewport, 4 / 3), const Size(400, 300));
    expect(fittedSize(const Size(400, 200), 4 / 3).width, closeTo(266.67, 0.01));
  });

  group('clampPan', () {
    test('at 1x nothing can move', () {
      expect(clampPan(const Offset(50, 50), 1, viewport, content), Offset.zero);
    });

    test('zoomed, the picture may move by what sticks out past the viewport, and no more', () {
      // 3x: 1200 wide in a 400 viewport = 400 each way; 900 tall in 800 = 50 each way.
      expect(clampPan(const Offset(900, -900), 3, viewport, content), const Offset(400, -50));
      expect(clampPan(const Offset(-120, 20), 3, viewport, content), const Offset(-120, 20));
    });
  });

  group('splitHorizontal', () {
    test('a move inside the picture is all pan', () {
      final ({Offset pan, double overflow}) r = splitHorizontal(100, Offset.zero, 3, viewport, content);
      expect(r.pan.dx, 100);
      expect(r.overflow, 0);
    });

    test('what goes past the picture edge becomes page movement', () {
      final ({Offset pan, double overflow}) r = splitHorizontal(100, const Offset(350, 10), 3, viewport, content);
      expect(r.pan, const Offset(400, 10));
      expect(r.overflow, 50);
      final ({Offset pan, double overflow}) left = splitHorizontal(-100, const Offset(-380, 0), 3, viewport, content);
      expect(left.pan.dx, -400);
      expect(left.overflow, -80);
    });
  });

  group('targetPage', () {
    test('settles on the nearest page when slow', () {
      expect(targetPage(1.4, 0, 1, 5), 1);
      expect(targetPage(1.6, 100, 1, 5), 2);
    });

    test('a fast fling turns one page even when dragged little', () {
      expect(targetPage(1.1, -900, 1, 5), 2);
      expect(targetPage(0.9, 900, 1, 5), 0);
    });

    test('never leaves the strip', () {
      expect(targetPage(0, 900, 0, 3), 0);
      expect(targetPage(2, -900, 2, 3), 2);
    });
  });

  group('dragPosition', () {
    test('inside the strip it follows the finger exactly', () {
      expect(dragPosition(1, 0.25, 3), 1.25);
    });

    test('past the first and last page it resists, and coming back is full speed', () {
      expect(dragPosition(0, -0.2, 3), closeTo(-0.07, 1e-9));
      expect(dragPosition(2, 0.2, 3), closeTo(2.07, 1e-9));
      expect(dragPosition(-0.07, 0.07, 3), closeTo(0, 1e-9));
    });
  });

  test('zooming around a point keeps that point under the fingers and respects the limits', () {
    final ({double zoom, Offset pan}) r = zoomAround(
      focal: const Offset(100, 0),
      startPan: Offset.zero,
      startZoom: 1,
      zoom: 2,
      shift: Offset.zero,
      viewport: viewport,
      content: content,
    );
    expect(r.zoom, 2);
    expect(r.pan.dx, -100);
    expect(zoomAround(focal: Offset.zero, startPan: Offset.zero, startZoom: 1, zoom: 50, shift: Offset.zero, viewport: viewport, content: content).zoom, maxZoom);
    expect(zoomAround(focal: Offset.zero, startPan: Offset.zero, startZoom: 2, zoom: 0.1, shift: Offset.zero, viewport: viewport, content: content).zoom, minZoom);
  });
}
