import 'dart:math' as math;
import 'dart:ui';

const double minZoom = 1;
const double maxZoom = 6;

Size fittedSize(Size viewport, double aspect) {
  if (viewport.isEmpty || aspect <= 0) return viewport;
  final double byWidth = viewport.width / aspect;
  return byWidth <= viewport.height
      ? Size(viewport.width, byWidth)
      : Size(viewport.height * aspect, viewport.height);
}

double _room(double content, double viewport, double zoom) =>
    math.max(0, (content * zoom - viewport) / 2);

Offset clampPan(Offset pan, double zoom, Size viewport, Size content) {
  final double roomX = _room(content.width, viewport.width, zoom);
  final double roomY = _room(content.height, viewport.height, zoom);
  return Offset(pan.dx.clamp(-roomX, roomX), pan.dy.clamp(-roomY, roomY));
}

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
