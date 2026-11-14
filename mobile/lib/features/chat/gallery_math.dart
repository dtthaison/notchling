import 'dart:math' as math;
import 'dart:ui';

/// The pure arithmetic of the picture gallery's gestures, kept apart from the widget so it can be tested.

const double minZoom = 1;
const double maxZoom = 6;
const double doubleTapZoom = 2.5;

/// A fling faster than this (px/s) turns the page even when it was dragged less than halfway.
const double flingVelocity = 400;

/// Where a picture sits at zoom 1 when it is fitted into [viewport] (its aspect is width over height).
Size fittedSize(Size viewport, double aspect) {
  if (viewport.isEmpty || aspect <= 0) return viewport;
  final double byWidth = viewport.width / aspect;
  return byWidth <= viewport.height ? Size(viewport.width, byWidth) : Size(viewport.height * aspect, viewport.height);
}

/// The most the picture can be moved from the centre on one axis at [zoom]: what is wider than the viewport.
double _room(double content, double viewport, double zoom) => math.max(0, (content * zoom - viewport) / 2);

/// [pan] (the picture centre's offset from the viewport centre) kept inside what the zoomed picture covers, so no
/// empty edge shows beside it.
Offset clampPan(Offset pan, double zoom, Size viewport, Size content) {
  final double roomX = _room(content.width, viewport.width, zoom);
  final double roomY = _room(content.height, viewport.height, zoom);
  return Offset(pan.dx.clamp(-roomX, roomX), pan.dy.clamp(-roomY, roomY));
}

/// A horizontal finger move of [dx] on a zoomed picture: the part the picture can still pan, and the part left over
/// once its edge is reached, which moves the page instead.
({Offset pan, double overflow}) splitHorizontal(double dx, Offset pan, double zoom, Size viewport, Size content) {
  final double room = _room(content.width, viewport.width, zoom);
  final double wanted = pan.dx + dx;
  final double kept = wanted.clamp(-room, room);
  return (pan: Offset(kept, pan.dy), overflow: wanted - kept);
}

/// The page to settle on after a drag: [position] is where the strip is now (in pages, fractional), [current] the
/// page the drag started on and [velocityX] the finger's speed (negative = towards the next page).
int targetPage(double position, double velocityX, int current, int count) {
  final int target = velocityX.abs() > flingVelocity ? current + (velocityX < 0 ? 1 : -1) : position.round();
  return target.clamp(0, math.max(0, count - 1));
}

/// A drag of [deltaPages] pages from [position] with a rubber band past the first and last page: movement back
/// towards the strip is full speed, movement further out is resisted.
double dragPosition(double position, double deltaPages, int count) {
  const double resistance = 0.35;
  final double last = math.max(0, count - 1).toDouble();
  double at = position;
  double left = deltaPages;
  if (at < 0) {
    if (left <= 0) return at + left * resistance;
    final double back = math.min(left, -at);
    at += back;
    left -= back;
  } else if (at > last) {
    if (left >= 0) return at + left * resistance;
    final double back = math.max(left, last - at);
    at += back;
    left -= back;
  }
  final double next = at + left;
  if (next >= 0 && next <= last) return next;
  final double edge = next < 0 ? 0 : last;
  return edge + (next - edge) * resistance;
}

/// The zoom and pan after pinching so the point [focal] (from the viewport centre) stays under the fingers.
({double zoom, Offset pan}) zoomAround({
  required Offset focal,
  required Offset startPan,
  required double startZoom,
  required double zoom,
  required Offset shift,
  required Size viewport,
  required Size content,
}) {
  final double next = zoom.clamp(minZoom, maxZoom);
  final Offset pan = focal - (focal - startPan) * (next / startZoom) + shift;
  return (zoom: next, pan: clampPan(pan, next, viewport, content));
}
