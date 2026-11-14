import 'dart:async';
import 'dart:math' as math;
import 'dart:ui' as ui;
import 'dart:collection';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/tokens.dart';
import '../../core/net/bridge_client.dart';
import '../../core/protocol/models.dart';
import '../../l10n/app_localizations.dart';
import '../connection/connection.dart';
import 'chat_cache.dart';
import 'gallery_math.dart';
import 'markup_editor.dart';
import 'photo_send.dart';

/// Widths the phone asks the Mac for: a thumbnail under a message, and the full-screen view.
const int thumbnailWidth = 480;
const int viewerWidth = 1600;

/// Fetches pictures once: memory first, then the phone's disk cache, then the Mac. Two widgets asking for the
/// same picture share one request.
class ChatPictureLoader {
  ChatPictureLoader(this._cache, this._client, {this.memoryLimit = 48});
  final ChatCache _cache;
  final BridgeClient? Function() _client;
  final int memoryLimit;

  final LinkedHashMap<String, Uint8List> _memory = LinkedHashMap<String, Uint8List>();
  final Map<String, Future<Uint8List>> _inFlight = <String, Future<Uint8List>>{};

  Future<Uint8List> load(String sessionId, ChatImage image, {int width = thumbnailWidth}) {
    final String key = '$sessionId|${image.id}|$width';
    final Uint8List? hit = _memory.remove(key);
    if (hit != null) {
      _memory[key] = hit;
      return Future<Uint8List>.value(hit);
    }
    return _inFlight[key] ??= _fetch(key, sessionId, image, width).whenComplete(() {
      // A block body: returning the removed future would make this wait on itself.
      _inFlight.remove(key);
    });
  }

  Future<Uint8List> _fetch(String key, String sessionId, ChatImage image, int width) async {
    Uint8List? bytes = await _cache.loadImage(sessionId, image.id, width);
    if (bytes == null) {
      final BridgeClient? client = _client();
      if (client == null || !client.isConnected) throw const BridgeException('disconnected', 'Chưa kết nối tới Mac.');
      bytes = await client.chatImage(sessionId, image.id, maxWidth: width);
      unawaited(_cache.saveImage(sessionId, image.id, width, bytes));
    }
    _memory[key] = bytes;
    while (_memory.length > memoryLimit) {
      _memory.remove(_memory.keys.first);
    }
    return bytes;
  }
}

final Provider<ChatPictureLoader> chatPictureLoaderProvider = Provider<ChatPictureLoader>((Ref ref) {
  final ChatCache cache = ref.watch(chatCacheProvider);
  return ChatPictureLoader(cache, () => ref.read(bridgeClientProvider));
});

/// The pictures of one message, as thumbnails. Each loads when it is built (so only the ones scrolled into
/// view cost anything), shows its shape while loading, and says so when it failed.
class MessagePictures extends StatelessWidget {
  const MessagePictures({super.key, required this.sessionId, required this.images, this.alignEnd = false});
  final String sessionId;
  final List<ChatImage> images;
  final bool alignEnd;

  @override
  Widget build(BuildContext context) {
    if (images.isEmpty) return const SizedBox.shrink();
    return Padding(
      padding: const EdgeInsets.only(top: Space.sm),
      child: Wrap(
        spacing: Space.sm,
        runSpacing: Space.sm,
        alignment: alignEnd ? WrapAlignment.end : WrapAlignment.start,
        children: <Widget>[
          for (final ChatImage image in images)
            PictureThumbnail(key: ValueKey<String>(image.id), sessionId: sessionId, image: image, siblings: images),
        ],
      ),
    );
  }
}

enum PictureLoad { loading, ready, failed }

class PictureThumbnail extends ConsumerStatefulWidget {
  const PictureThumbnail({super.key, required this.sessionId, required this.image, this.siblings = const <ChatImage>[]});
  final String sessionId;
  final ChatImage image;

  /// Every picture of the same message, so the viewer can swipe from this one to the next.
  final List<ChatImage> siblings;

  @override
  ConsumerState<PictureThumbnail> createState() => _PictureThumbnailState();
}

class _PictureThumbnailState extends ConsumerState<PictureThumbnail> {
  PictureLoad _phase = PictureLoad.loading;
  Uint8List? _bytes;

  static const double _maxWidth = 220;
  static const double _maxHeight = 260;

  @override
  void initState() {
    super.initState();
    unawaited(_load());
  }

  Future<void> _load() async {
    if (_phase != PictureLoad.loading) setState(() => _phase = PictureLoad.loading);
    try {
      final Uint8List bytes = await ref.read(chatPictureLoaderProvider).load(widget.sessionId, widget.image);
      if (!mounted) return;
      setState(() {
        _bytes = bytes;
        _phase = PictureLoad.ready;
      });
    } on BridgeException catch (error) {
      debugPrint('PictureThumbnail(${widget.image.id}): ${error.code} ${error.message}');
      if (mounted) setState(() => _phase = PictureLoad.failed);
    }
  }

  Size get _box {
    final double aspect = widget.image.aspect ?? 4 / 3;
    double width = _maxWidth;
    double height = width / aspect;
    if (height > _maxHeight) {
      height = _maxHeight;
      width = height * aspect;
    }
    return Size(width.clamp(72, _maxWidth), height.clamp(48, _maxHeight));
  }

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final Size box = _box;
    final Widget child = switch (_phase) {
      // An unknown shape gets the 4:3 guess; cover would crop a tall screenshot to its middle, so show it whole.
      PictureLoad.ready => Image.memory(_bytes!,
          fit: widget.image.aspect == null ? BoxFit.contain : BoxFit.cover,
          gaplessPlayback: true,
          cacheWidth: (box.width * 2).round()),
      PictureLoad.loading => const Center(child: SizedBox(width: 18, height: 18, child: CircularProgressIndicator(strokeWidth: 2))),
      PictureLoad.failed => Center(
          child: Column(mainAxisSize: MainAxisSize.min, children: <Widget>[
            Icon(Icons.broken_image_outlined, size: 20, color: Kit.of(context).textMuted),
            const SizedBox(height: 2),
            Text(l10n.pictureFailed, style: Kit.of(context).labelSm, textAlign: TextAlign.center),
          ]),
        ),
    };
    return Semantics(
      button: true,
      label: l10n.pictureOpen,
      child: InkWell(
        borderRadius: BorderRadius.circular(Radii.md),
        onTap: switch (_phase) {
          PictureLoad.ready => () => unawaited(Navigator.of(context).push(MaterialPageRoute<void>(
                fullscreenDialog: true,
                builder: (_) => PictureViewer(
                  sessionId: widget.sessionId,
                  images: widget.siblings.isEmpty ? <ChatImage>[widget.image] : widget.siblings,
                  initial: widget.siblings.indexWhere((ChatImage other) => other.id == widget.image.id).clamp(0, widget.siblings.length),
                  preview: _bytes!,
                ),
              ))),
          PictureLoad.failed => () => unawaited(_load()),
          PictureLoad.loading => null,
        },
        child: Container(
          width: box.width,
          height: box.height,
          clipBehavior: Clip.antiAlias,
          decoration: BoxDecoration(color: Kit.of(context).sidebar, borderRadius: BorderRadius.circular(Radii.md), border: Border.all(color: Kit.of(context).hairline)),
          child: child,
        ),
      ),
    );
  }
}

/// Full screen: drag sideways through the message's pictures, pinch to zoom one. One handler owns every gesture (a
/// PageView plus InteractiveViewer fight over the same drag, and pinch lost on a phone): two fingers zoom and pan,
/// one finger at 1x turns the page, one finger zoomed pans the picture and, once its edge is reached, carries on
/// into the next page.
class PictureViewer extends ConsumerStatefulWidget {
  const PictureViewer({super.key, required this.sessionId, required this.images, this.initial = 0, this.preview});
  final String sessionId;
  final List<ChatImage> images;
  final int initial;

  /// The tapped picture's thumbnail, shown until its large version arrives.
  final Uint8List? preview;

  @override
  ConsumerState<PictureViewer> createState() => _PictureViewerState();
}

class _PictureViewerState extends ConsumerState<PictureViewer> with SingleTickerProviderStateMixin {
  late final AnimationController _anim;

  /// Where the strip is, in pages (fractional while dragging or settling).
  late double _position = widget.initial.toDouble();
  late int _index = widget.initial;
  double _zoom = 1;
  Offset _pan = Offset.zero;
  Size _viewport = Size.zero;

  // What the running animation moves from and to.
  double _fromPosition = 0, _toPosition = 0, _fromZoom = 1, _toZoom = 1;
  Offset _fromPan = Offset.zero, _toPan = Offset.zero;

  // The gesture in progress.
  int _pointers = 0;
  double _startZoom = 1;
  Offset _startPan = Offset.zero;
  Offset _startFocal = Offset.zero;
  Offset _lastFocal = Offset.zero;
  Offset _doubleTapAt = Offset.zero;

  /// The bytes each page shows now (large when it arrived), for Reply and Mark up.
  final Map<int, Uint8List> _bytes = <int, Uint8List>{};

  int get _count => widget.images.length;

  @override
  void initState() {
    super.initState();
    _anim = AnimationController(vsync: this, duration: const Duration(milliseconds: 250))..addListener(_tick);
  }

  @override
  void dispose() {
    _anim.dispose();
    super.dispose();
  }

  Size _contentOf(int index) => fittedSize(_viewport, widget.images[index].aspect ?? (_viewport.width / math.max(1, _viewport.height)));
  Offset get _center => Offset(_viewport.width / 2, _viewport.height / 2);

  // MARK: gestures

  void _scaleStart(ScaleStartDetails details) {
    _anim.stop();
    _settleIndex();
    _beginGesture(details.pointerCount, details.localFocalPoint);
  }

  /// Called again whenever the number of fingers changes, so the new gesture starts from where the old one ended.
  void _beginGesture(int pointers, Offset focal) {
    _pointers = pointers;
    _startZoom = _zoom;
    _startPan = _pan;
    _startFocal = focal;
    _lastFocal = focal;
  }

  /// A stopped animation can leave the strip between pages: the page it is closer to is the current one.
  void _settleIndex() {
    final int near = _position.round().clamp(0, _count - 1);
    if (near != _index) {
      _index = near;
      _zoom = 1;
      _pan = Offset.zero;
    }
  }

  void _scaleUpdate(ScaleUpdateDetails details) {
    if (_viewport.isEmpty) return;
    if (details.pointerCount != _pointers) _beginGesture(details.pointerCount, details.localFocalPoint);
    final Size content = _contentOf(_index);
    if (details.pointerCount >= 2) {
      final ({double zoom, Offset pan}) next = zoomAround(
        focal: _startFocal - _center,
        startPan: _startPan,
        startZoom: _startZoom,
        zoom: _startZoom * details.scale,
        shift: details.localFocalPoint - _startFocal,
        viewport: _viewport,
        content: content,
      );
      setState(() {
        _zoom = next.zoom;
        _pan = next.pan;
      });
      return;
    }
    final Offset delta = details.localFocalPoint - _lastFocal;
    _lastFocal = details.localFocalPoint;
    setState(() {
      if (_zoom <= 1.01) {
        _position = dragPosition(_position, -delta.dx / _viewport.width, _count);
        return;
      }
      double dx = delta.dx;
      // A page left part-way across by an earlier overflow comes back before the picture pans again.
      final double displaced = (_position - _index) * _viewport.width;
      if (displaced != 0 && dx != 0 && (displaced > 0) == (dx > 0)) {
        final double back = math.min(dx.abs(), displaced.abs()) * dx.sign;
        _position -= back / _viewport.width;
        dx -= back;
      }
      // Pan inside the picture; what is left of the horizontal move once its edge is reached turns the page.
      final ({Offset pan, double overflow}) split = splitHorizontal(dx, _pan, _zoom, _viewport, content);
      _pan = clampPan(Offset(split.pan.dx, split.pan.dy + delta.dy), _zoom, _viewport, content);
      if (split.overflow != 0) _position = dragPosition(_position, -split.overflow / _viewport.width, _count);
    });
  }

  void _scaleEnd(ScaleEndDetails details) {
    _pointers = 0;
    final double velocity = details.velocity.pixelsPerSecond.dx;
    final bool offCenter = (_position - _index).abs() > 0.001;
    if (offCenter) {
      final int target = targetPage(_position, velocity, _index, _count);
      _animateTo(position: target.toDouble(), zoom: target == _index ? _zoom : 1, pan: target == _index ? _pan : Offset.zero);
    } else if (_zoom < 1.01) {
      _animateTo(position: _position, zoom: 1, pan: Offset.zero);
    }
  }

  void _doubleTapDown(TapDownDetails details) => _doubleTapAt = details.localPosition;

  void _doubleTap() {
    if (_viewport.isEmpty || (_position - _index).abs() > 0.001) return;
    if (_zoom > 1.01) {
      _animateTo(position: _position, zoom: 1, pan: Offset.zero);
      return;
    }
    final ({double zoom, Offset pan}) next = zoomAround(
      focal: _doubleTapAt - _center,
      startPan: _pan,
      startZoom: _zoom,
      zoom: doubleTapZoom,
      shift: Offset.zero,
      viewport: _viewport,
      content: _contentOf(_index),
    );
    _animateTo(position: _position, zoom: next.zoom, pan: next.pan);
  }

  // MARK: animation

  void _animateTo({required double position, required double zoom, required Offset pan}) {
    _fromPosition = _position;
    _toPosition = position;
    _fromZoom = _zoom;
    _toZoom = zoom;
    _fromPan = _pan;
    _toPan = pan;
    if (MediaQuery.disableAnimationsOf(context)) {
      _anim.value = 1;
      _tick();
      _finish();
      return;
    }
    _anim
      ..value = 0
      ..animateTo(1, curve: Curves.easeOutCubic).whenCompleteOrCancel(() {
        if (mounted && _anim.value == 1) _finish();
      });
  }

  void _tick() {
    final double t = _anim.value;
    setState(() {
      _position = ui.lerpDouble(_fromPosition, _toPosition, t)!;
      _zoom = ui.lerpDouble(_fromZoom, _toZoom, t)!;
      _pan = Offset.lerp(_fromPan, _toPan, t)!;
    });
  }

  /// The strip stopped on a page: the one left behind forgets its zoom.
  void _finish() {
    final int landed = _toPosition.round().clamp(0, _count - 1);
    setState(() {
      if (landed != _index) {
        _index = landed;
        _zoom = 1;
        _pan = Offset.zero;
      }
      _position = landed.toDouble();
    });
  }

  // MARK: actions

  Uint8List? get _currentBytes => _bytes[_index];

  Future<void> _reply() async {
    final Uint8List? bytes = _currentBytes;
    if (bytes == null) return;
    final ScaffoldMessengerState messenger = ScaffoldMessenger.of(context);
    final AppL10n l10n = AppL10n.of(context);
    final NavigatorState navigator = Navigator.of(context);
    final bool sent = await previewAndSend(context, ref, widget.sessionId, bytes);
    if (!sent) return;
    navigator.pop();
    messenger.showSnackBar(SnackBar(content: Text(l10n.photoSent)));
  }

  Future<void> _markup() async {
    final Uint8List? bytes = _currentBytes;
    if (bytes == null) return;
    final ScaffoldMessengerState messenger = ScaffoldMessenger.of(context);
    final AppL10n l10n = AppL10n.of(context);
    final NavigatorState navigator = Navigator.of(context);
    final bool? sent = await navigator.push<bool>(MaterialPageRoute<bool>(
      fullscreenDialog: true,
      builder: (_) => MarkupEditor(sessionId: widget.sessionId, bytes: bytes),
    ));
    if (sent != true) return;
    navigator.pop();
    messenger.showSnackBar(SnackBar(content: Text(l10n.photoSent)));
  }

  // MARK: build

  Widget _pageAt(int index) {
    final bool current = index == _index;
    final double x = (index - _position) * _viewport.width;
    final Matrix4 matrix = Matrix4.translationValues(x + (current ? _pan.dx : 0), current ? _pan.dy : 0, 0);
    if (current) matrix.scaleByDouble(_zoom, _zoom, 1, 1);
    return Transform(
      key: ValueKey<String>('page-${widget.images[index].id}'),
      transform: matrix,
      alignment: Alignment.center,
      child: _PicturePage(
        sessionId: widget.sessionId,
        image: widget.images[index],
        preview: index == widget.initial ? widget.preview : null,
        onBytes: (Uint8List bytes) {
          if (!mounted) return;
          // The state is read when a button is pressed, so a rebuild is only needed to enable the buttons.
          final bool first = !_bytes.containsKey(index);
          _bytes[index] = bytes;
          if (first && index == _index) setState(() {});
        },
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final Kit kit = Kit.of(context);
    final bool ready = _currentBytes != null;
    return Scaffold(
      backgroundColor: Kit.dark.sidebar,
      appBar: AppBar(
        backgroundColor: Kit.dark.sidebar,
        leading: IconButton(tooltip: l10n.pictureClose, icon: const Icon(Icons.close), onPressed: () => Navigator.of(context).pop()),
        title: _count > 1 ? Text('${_index + 1} / $_count', style: kit.label) : null,
        centerTitle: true,
      ),
      body: Stack(fit: StackFit.expand, children: <Widget>[
        LayoutBuilder(builder: (BuildContext context, BoxConstraints constraints) {
          _viewport = constraints.biggest;
          return ClipRect(
            child: GestureDetector(
              key: const ValueKey<String>('picture-gallery'),
              behavior: HitTestBehavior.opaque,
              onScaleStart: _scaleStart,
              onScaleUpdate: _scaleUpdate,
              onScaleEnd: _scaleEnd,
              onDoubleTapDown: _doubleTapDown,
              onDoubleTap: _doubleTap,
              child: Stack(fit: StackFit.expand, children: <Widget>[
                for (int i = 0; i < _count; i++)
                  // Only the pages that can be on screen are built; the others would load pictures for nothing.
                  if ((i - _position).abs() < 1.5) _pageAt(i),
              ]),
            ),
          );
        }),
        Positioned(
          left: 0,
          right: 0,
          bottom: 0,
          child: DecoratedBox(
            decoration: BoxDecoration(
              gradient: LinearGradient(begin: Alignment.bottomCenter, end: Alignment.topCenter, colors: <Color>[Kit.dark.sidebar.withValues(alpha: 0.92), Kit.dark.sidebar.withValues(alpha: 0)]),
            ),
            child: SafeArea(
              top: false,
              child: Padding(
                padding: const EdgeInsets.fromLTRB(Space.lg, Space.xl, Space.lg, Space.md),
                child: Row(children: <Widget>[
                  Expanded(
                    child: FilledButton.icon(
                      key: const ValueKey<String>('picture-reply'),
                      onPressed: ready ? () => unawaited(_reply()) : null,
                      icon: const Icon(Icons.reply, size: 20),
                      label: Text(l10n.pictureReply),
                    ),
                  ),
                  const SizedBox(width: Space.md),
                  Expanded(
                    child: OutlinedButton.icon(
                      key: const ValueKey<String>('picture-markup'),
                      onPressed: ready ? () => unawaited(_markup()) : null,
                      icon: const Icon(Icons.draw_outlined, size: 20),
                      label: Text(l10n.pictureMarkup),
                    ),
                  ),
                ]),
              ),
            ),
          ),
        ),
      ]),
    );
  }
}

/// One picture in the viewer: thumbnail first (from memory or disk, usually already loaded), then the large one.
class _PicturePage extends ConsumerStatefulWidget {
  const _PicturePage({required this.sessionId, required this.image, required this.preview, required this.onBytes});
  final String sessionId;
  final ChatImage image;
  final Uint8List? preview;

  /// Each time better bytes arrive: the thumbnail, then the large version.
  final ValueChanged<Uint8List> onBytes;

  @override
  ConsumerState<_PicturePage> createState() => _PicturePageState();
}

class _PicturePageState extends ConsumerState<_PicturePage> {
  late Uint8List? _shown = widget.preview;
  bool _sharp = false;
  bool _failed = false;

  @override
  void initState() {
    super.initState();
    final Uint8List? first = _shown;
    if (first != null) WidgetsBinding.instance.addPostFrameCallback((_) => widget.onBytes(first));
    unawaited(_load());
  }

  Future<void> _load() async {
    final ChatPictureLoader loader = ref.read(chatPictureLoaderProvider);
    if (_shown == null) {
      try {
        final Uint8List thumb = await loader.load(widget.sessionId, widget.image);
        if (mounted && !_sharp) {
          setState(() => _shown = thumb);
          widget.onBytes(thumb);
        }
      } on BridgeException catch (error) {
        debugPrint('PictureViewer thumbnail(${widget.image.id}): ${error.code} ${error.message}');
      }
    }
    try {
      final Uint8List bytes = await loader.load(widget.sessionId, widget.image, width: viewerWidth);
      if (mounted) {
        setState(() {
          _shown = bytes;
          _sharp = true;
        });
        widget.onBytes(bytes);
      }
    } on BridgeException catch (error) {
      debugPrint('PictureViewer(${widget.image.id}): ${error.code} ${error.message}');
      if (mounted && _shown == null) setState(() => _failed = true);
    }
  }

  @override
  Widget build(BuildContext context) {
    final Uint8List? shown = _shown;
    final Widget picture = shown != null
        ? Center(child: Image.memory(shown, fit: BoxFit.contain, gaplessPlayback: true))
        : Center(
            child: _failed
                ? Text(AppL10n.of(context).pictureFailed, style: Kit.of(context).labelSm)
                : const SizedBox(width: 22, height: 22, child: CircularProgressIndicator(strokeWidth: 2)),
          );
    return Stack(fit: StackFit.expand, children: <Widget>[
      picture,
      if (!_sharp && shown != null) const Positioned(top: 0, left: 0, right: 0, child: LinearProgressIndicator(minHeight: 2)),
    ]);
  }
}
