import 'dart:async';
import 'dart:math' as math;
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/tokens.dart';
import '../../app/widgets/common.dart';
import '../../l10n/app_localizations.dart';
import 'gallery_math.dart';
import 'photo_send.dart';

enum MarkTool { pen, ellipse, rectangle, arrow }

const List<Color> markColors = <Color>[
  MarkInk.red,
  MarkInk.yellow,
  MarkInk.white,
];

class Mark {
  Mark(this.tool, this.color, Offset start) : points = <Offset>[start];
  final MarkTool tool;
  final Color color;
  final List<Offset> points;

  Offset get start => points.first;
  Offset get end => points.last;

  void extend(Offset to) {
    if (tool == MarkTool.pen) {
      points.add(to);
    } else if (points.length == 1) {
      points.add(to);
    } else {
      points[1] = to;
    }
  }
}

class MarkupBoard extends ChangeNotifier {
  final List<Mark> marks = <Mark>[];

  void add(Mark mark) {
    marks.add(mark);
    notifyListeners();
  }

  void changed() => notifyListeners();

  void removeLast() {
    if (marks.isEmpty) return;
    marks.removeLast();
    notifyListeners();
  }

  void clear() {
    if (marks.isEmpty) return;
    marks.clear();
    notifyListeners();
  }
}

double markStrokeWidth(Size image) =>
    math.max(2, math.max(image.width, image.height) * 0.006);

void paintMarks(Canvas canvas, List<Mark> marks, double strokeWidth) {
  for (final Mark mark in marks) {
    final Paint paint = Paint()
      ..color = mark.color
      ..style = PaintingStyle.stroke
      ..strokeWidth = strokeWidth
      ..strokeCap = StrokeCap.round
      ..strokeJoin = StrokeJoin.round
      ..isAntiAlias = true;
    switch (mark.tool) {
      case MarkTool.pen:
        if (mark.points.length == 1) {
          canvas.drawCircle(
            mark.start,
            strokeWidth / 2,
            paint..style = PaintingStyle.fill,
          );
        } else {
          final Path path = Path()..moveTo(mark.start.dx, mark.start.dy);
          for (final Offset point in mark.points.skip(1)) {
            path.lineTo(point.dx, point.dy);
          }
          canvas.drawPath(path, paint);
        }
      case MarkTool.ellipse:
        canvas.drawOval(Rect.fromPoints(mark.start, mark.end), paint);
      case MarkTool.rectangle:
        canvas.drawRect(Rect.fromPoints(mark.start, mark.end), paint);
      case MarkTool.arrow:
        canvas.drawLine(mark.start, mark.end, paint);
        final Offset along = mark.end - mark.start;
        if (along.distance > strokeWidth * 2) {
          final double angle = math.atan2(along.dy, along.dx);
          final double head = math.min(strokeWidth * 6, along.distance / 2);
          for (final double side in <double>[
            math.pi * 5 / 6,
            -math.pi * 5 / 6,
          ]) {
            canvas.drawLine(
              mark.end,
              mark.end +
                  Offset(math.cos(angle + side), math.sin(angle + side)) * head,
              paint,
            );
          }
        }
    }
  }
}

const int uploadCapBytes = 6 * 1024 * 1024;
const int _uploadMargin = 64 * 1024;
const int _shrinkSteps = 8;

class MarkupTooBig implements Exception {
  const MarkupTooBig();
}

Future<Uint8List> _encodeMarkup(ui.Image image, List<Mark> marks, double scale) async {
  final int width = math.max(1, (image.width * scale).round());
  final int height = math.max(1, (image.height * scale).round());
  final ui.PictureRecorder recorder = ui.PictureRecorder();
  final Canvas canvas = Canvas(recorder, Rect.fromLTWH(0, 0, width.toDouble(), height.toDouble()));
  canvas.scale(width / image.width, height / image.height);
  canvas.drawImage(image, Offset.zero, Paint()..filterQuality = FilterQuality.high);
  paintMarks(canvas, marks, markStrokeWidth(Size(image.width.toDouble(), image.height.toDouble())));
  final ui.Image flat = await recorder.endRecording().toImage(width, height);
  try {
    final ByteData? data = await flat.toByteData(format: ui.ImageByteFormat.png);
    if (data == null) throw StateError('png');
    return data.buffer.asUint8List(data.offsetInBytes, data.lengthInBytes);
  } finally {
    flat.dispose();
  }
}

Future<Uint8List> renderMarkup(ui.Image image, List<Mark> marks, {int maxBytes = uploadCapBytes - _uploadMargin}) async {
  double scale = 1;
  for (int step = 0; step <= _shrinkSteps; step++) {
    final Uint8List png = await _encodeMarkup(image, marks, scale);
    if (png.length <= maxBytes) return png;
    scale *= math.max(0.5, math.min(0.85, math.sqrt(maxBytes / png.length) * 0.95));
  }
  throw const MarkupTooBig();
}

class MarkupEditor extends ConsumerStatefulWidget {
  const MarkupEditor({
    super.key,
    required this.sessionId,
    required this.bytes,
    this.board,
  });
  final String sessionId;
  final Uint8List bytes;
  final MarkupBoard? board;

  @override
  ConsumerState<MarkupEditor> createState() => _MarkupEditorState();
}

class _MarkupEditorState extends ConsumerState<MarkupEditor> {
  late final MarkupBoard _board = widget.board ?? MarkupBoard();
  ui.Image? _image;
  bool _decodeFailed = false;
  bool _rendering = false;
  bool _renderFailed = false;
  MarkTool _tool = MarkTool.pen;
  Color _color = markColors.first;

  double _zoom = 1;
  Offset _pan = Offset.zero;
  Size _canvas = Size.zero;

  Mark? _drawing;

  int _down = 0;
  bool _pinched = false;
  int _lastPointers = 0;
  double _startZoom = 1;
  Offset _startPan = Offset.zero;
  Offset _startFocal = Offset.zero;

  @override
  void initState() {
    super.initState();
    unawaited(_decode());
  }

  @override
  void dispose() {
    _image?.dispose();
    if (widget.board == null) _board.dispose();
    super.dispose();
  }

  Future<void> _decode() async {
    try {
      final ui.Codec codec = await ui.instantiateImageCodec(widget.bytes);
      final ui.FrameInfo frame = await codec.getNextFrame();
      codec.dispose();
      if (!mounted) {
        frame.image.dispose();
        return;
      }
      setState(() => _image = frame.image);
    } on Exception catch (error) {
      debugPrint('MarkupEditor.decode: $error');
      if (mounted) setState(() => _decodeFailed = true);
    }
  }

  Size get _imageSize =>
      Size(_image!.width.toDouble(), _image!.height.toDouble());
  Size get _fitted => fittedSize(_canvas, _imageSize.width / _imageSize.height);

  Offset _toImage(Offset local) {
    final Offset fromCenter =
        local - Offset(_canvas.width / 2, _canvas.height / 2);
    final Offset unzoomed = (fromCenter - _pan) / _zoom;
    return (unzoomed + Offset(_fitted.width / 2, _fitted.height / 2)) *
        (_imageSize.width / _fitted.width);
  }

  void _scaleStart(ScaleStartDetails details) {
    if (details.pointerCount >= 2) _discardStray();
    _lastPointers = details.pointerCount;
    _startZoom = _zoom;
    _startPan = _pan;
    _startFocal = details.localFocalPoint;
    if (details.pointerCount == 1 && !_pinched) {
      _drawing = Mark(_tool, _color, _toImage(details.localFocalPoint));
      _board.add(_drawing!);
    }
  }

  void _scaleUpdate(ScaleUpdateDetails details) {
    if (details.pointerCount >= 2) {
      _discardStray();
      if (_lastPointers < 2) {
        _startZoom = _zoom;
        _startPan = _pan;
        _startFocal = details.localFocalPoint;
      }
      final Offset center = Offset(_canvas.width / 2, _canvas.height / 2);
      final ({double zoom, Offset pan}) next = zoomAround(
        focal: _startFocal - center,
        startPan: _startPan,
        startZoom: _startZoom,
        zoom: _startZoom * details.scale,
        shift: details.localFocalPoint - _startFocal,
        viewport: _canvas,
        content: _fitted,
      );
      setState(() {
        _zoom = next.zoom;
        _pan = next.pan;
      });
    } else if (!_pinched) {
      final Mark? mark = _drawing;
      if (mark != null) {
        final Offset at = _toImage(details.localFocalPoint);
        final Size size = _imageSize;
        mark.extend(
          Offset(at.dx.clamp(0, size.width), at.dy.clamp(0, size.height)),
        );
        _board.changed();
      }
    }
    _lastPointers = details.pointerCount;
  }

  void _discardStray() {
    final Mark? stray = _drawing;
    if (stray == null) return;
    _board.marks.remove(stray);
    _drawing = null;
    _board.changed();
  }

  void _scaleEnd(ScaleEndDetails details) => _drawing = null;

  void _pointerLifted() {
    _down = math.max(0, _down - 1);
    if (_down == 0) _pinched = false;
  }

  Future<void> _next() async {
    final ui.Image? image = _image;
    if (image == null || _rendering) return;
    setState(() {
      _rendering = true;
      _renderFailed = false;
    });
    final Uint8List png;
    try {
      png = await renderMarkup(image, List<Mark>.of(_board.marks));
    } on Object catch (error) {
      debugPrint('MarkupEditor.render: $error');
      if (mounted) {
        setState(() {
          _rendering = false;
          _renderFailed = true;
        });
      }
      return;
    }
    if (!mounted) return;
    setState(() => _rendering = false);
    final bool sent = await previewAndSend(context, ref, widget.sessionId, png);
    if (sent && mounted) Navigator.of(context).pop(true);
  }

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final Kit kit = Kit.of(context);
    final ui.Image? image = _image;
    return Scaffold(
      backgroundColor: Kit.dark.sidebar,
      appBar: AppBar(
        backgroundColor: Kit.dark.sidebar,
        leading: IconButton(
          tooltip: l10n.appCancel,
          icon: const Icon(Icons.close),
          onPressed: _rendering ? null : () => Navigator.of(context).pop(false),
        ),
        title: Text(l10n.chatPictureMarkup, style: kit.label),
        centerTitle: true,
        actions: <Widget>[
          ListenableBuilder(
            listenable: _board,
            builder: (BuildContext context, Widget? _) => Row(
              children: <Widget>[
                IconButton(
                  key: const ValueKey<String>('markup-undo'),
                  tooltip: l10n.chatMarkUndo,
                  icon: const Icon(Icons.undo),
                  onPressed: _board.marks.isEmpty || _rendering
                      ? null
                      : _board.removeLast,
                ),
                IconButton(
                  key: const ValueKey<String>('markup-clear'),
                  tooltip: l10n.chatMarkClear,
                  icon: const Icon(Icons.layers_clear_outlined),
                  onPressed: _board.marks.isEmpty || _rendering
                      ? null
                      : _board.clear,
                ),
              ],
            ),
          ),
        ],
      ),
      body: SafeArea(
        child: Column(
          children: <Widget>[
            Expanded(
              child: image == null
                  ? Center(
                      child: _decodeFailed
                          ? Text(l10n.chatPictureFailed, style: kit.labelSm)
                          : const SizedBox(
                              width: 22,
                              height: 22,
                              child: CircularProgressIndicator(strokeWidth: 2),
                            ),
                    )
                  : LayoutBuilder(
                      builder:
                          (BuildContext context, BoxConstraints constraints) {
                            _canvas = constraints.biggest;
                            return ClipRect(
                              child: Listener(
                                onPointerDown: (_) {
                                  _down++;
                                  if (_down >= 2) {
                                    _pinched = true;
                                    _discardStray();
                                  }
                                },
                                onPointerUp: (_) => _pointerLifted(),
                                onPointerCancel: (_) => _pointerLifted(),
                                child: GestureDetector(
                                  key: const ValueKey<String>('markup-canvas'),
                                  behavior: HitTestBehavior.opaque,
                                  onScaleStart: _rendering ? null : _scaleStart,
                                  onScaleUpdate: _rendering
                                      ? null
                                      : _scaleUpdate,
                                  onScaleEnd: _rendering ? null : _scaleEnd,
                                  child: ListenableBuilder(
                                    listenable: _board,
                                    builder: (BuildContext context, Widget? _) =>
                                        CustomPaint(
                                          size: constraints.biggest,
                                          painter: _MarkupPainter(
                                            image,
                                            List<Mark>.of(_board.marks),
                                            _zoom,
                                            _pan,
                                          ),
                                        ),
                                  ),
                                ),
                              ),
                            );
                          },
                    ),
            ),
            if (_rendering) const LinearProgressIndicator(minHeight: 2),
            Container(
              color: Kit.dark.sidebar,
              padding: const EdgeInsets.fromLTRB(
                Space.md,
                Space.sm,
                Space.md,
                Space.md,
              ),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: <Widget>[
                  if (_renderFailed)
                    Padding(
                      padding: const EdgeInsets.only(bottom: Space.sm),
                      child: Text(
                        l10n.chatMarkFailed,
                        style: kit.meta.copyWith(color: kit.danger),
                      ),
                    ),
                  if (_rendering)
                    Padding(
                      padding: const EdgeInsets.only(bottom: Space.sm),
                      child: Text(l10n.chatMarkRendering, style: kit.meta),
                    ),
                  SingleChildScrollView(
                    scrollDirection: Axis.horizontal,
                    child: Row(
                      children: <Widget>[
                        _ToolButton(
                          tool: MarkTool.pen,
                          icon: Icons.edit,
                          label: l10n.chatMarkPen,
                          selected: _tool,
                          onTap: _pick,
                        ),
                        _ToolButton(
                          tool: MarkTool.ellipse,
                          icon: Icons.panorama_fish_eye,
                          label: l10n.chatMarkCircle,
                          selected: _tool,
                          onTap: _pick,
                        ),
                        _ToolButton(
                          tool: MarkTool.rectangle,
                          icon: Icons.crop_square,
                          label: l10n.chatMarkBox,
                          selected: _tool,
                          onTap: _pick,
                        ),
                        _ToolButton(
                          tool: MarkTool.arrow,
                          icon: Icons.north_east,
                          label: l10n.chatMarkArrow,
                          selected: _tool,
                          onTap: _pick,
                        ),
                        const SizedBox(width: Space.md),
                        for (final (Color color, String name)
                            in <(Color, String)>[
                              (markColors[0], l10n.chatMarkRed),
                              (markColors[1], l10n.chatMarkYellow),
                              (markColors[2], l10n.chatMarkWhite),
                            ])
                          _ColorDot(
                            color: color,
                            label: name,
                            selected: _color == color,
                            onTap: () => setState(() => _color = color),
                          ),
                      ],
                    ),
                  ),
                  const SizedBox(height: Space.sm),
                  Row(
                    children: <Widget>[
                      Expanded(
                        child: SecondaryButton(
                          label: l10n.appCancel,
                          onPressed: _rendering
                              ? null
                              : () => Navigator.of(context).pop(false),
                        ),
                      ),
                      const SizedBox(width: Space.md),
                      Expanded(
                        child: PrimaryButton(
                          key: const ValueKey<String>('markup-next'),
                          label: l10n.chatMarkNext,
                          onPressed: image == null || _rendering
                              ? null
                              : () => unawaited(_next()),
                        ),
                      ),
                    ],
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }

  void _pick(MarkTool tool) => setState(() => _tool = tool);
}

class _ToolButton extends StatelessWidget {
  const _ToolButton({
    required this.tool,
    required this.icon,
    required this.label,
    required this.selected,
    required this.onTap,
  });
  final MarkTool tool;
  final IconData icon;
  final String label;
  final MarkTool selected;
  final ValueChanged<MarkTool> onTap;

  @override
  Widget build(BuildContext context) {
    final Kit kit = Kit.of(context);
    final bool on = tool == selected;
    return Semantics(
      button: true,
      selected: on,
      label: label,
      child: InkWell(
        borderRadius: BorderRadius.circular(Radii.sm),
        onTap: () => onTap(tool),
        child: ConstrainedBox(
          constraints: const BoxConstraints(
            minWidth: Metric.minTarget,
            minHeight: Metric.minTarget,
          ),
          child: Container(
            margin: const EdgeInsets.only(right: Space.xs),
            padding: const EdgeInsets.symmetric(
              horizontal: Space.md,
              vertical: Space.sm,
            ),
            decoration: BoxDecoration(
              color: on ? kit.accentWash : null,
              borderRadius: BorderRadius.circular(Radii.sm),
            ),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: <Widget>[
                Icon(icon, size: 20, color: on ? kit.accentText : kit.textMuted),
                Text(
                  label,
                  style: kit.meta.copyWith(
                    color: on ? kit.accentText : kit.textMuted,
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class _ColorDot extends StatelessWidget {
  const _ColorDot({
    required this.color,
    required this.label,
    required this.selected,
    required this.onTap,
  });
  final Color color;
  final String label;
  final bool selected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final Kit kit = Kit.of(context);
    return Semantics(
      button: true,
      selected: selected,
      label: label,
      child: InkWell(
        customBorder: const CircleBorder(),
        onTap: onTap,
        child: SizedBox(
          width: Metric.minTarget,
          height: Metric.minTarget,
          child: Center(
            child: Container(
              width: Space.xl + Space.xs,
              height: Space.xl + Space.xs,
              decoration: BoxDecoration(
                color: color,
                shape: BoxShape.circle,
                border: Border.all(
                  color: selected ? kit.accent : kit.hairline,
                  width: selected ? 3 : 1,
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class _MarkupPainter extends CustomPainter {
  _MarkupPainter(this.image, this.marks, this.zoom, this.pan);
  final ui.Image image;
  final List<Mark> marks;
  final double zoom;
  final Offset pan;

  @override
  void paint(Canvas canvas, Size size) {
    final Size source = Size(image.width.toDouble(), image.height.toDouble());
    final Size fitted = fittedSize(size, source.width / source.height);
    final double toScreen = fitted.width / source.width;
    canvas
      ..translate(size.width / 2 + pan.dx, size.height / 2 + pan.dy)
      ..scale(zoom)
      ..translate(-fitted.width / 2, -fitted.height / 2)
      ..scale(toScreen);
    canvas.drawImage(
      image,
      Offset.zero,
      Paint()..filterQuality = FilterQuality.medium,
    );
    paintMarks(canvas, marks, markStrokeWidth(source));
  }

  @override
  bool shouldRepaint(_MarkupPainter old) => true;
}
