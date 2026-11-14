import 'dart:async';
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

/// Full screen: swipe sideways through the message's pictures, pinch to zoom one. While a picture is zoomed in,
/// a drag pans it instead of turning the page.
class PictureViewer extends StatefulWidget {
  const PictureViewer({super.key, required this.sessionId, required this.images, this.initial = 0, this.preview});
  final String sessionId;
  final List<ChatImage> images;
  final int initial;

  /// The tapped picture's thumbnail, shown until its large version arrives.
  final Uint8List? preview;

  @override
  State<PictureViewer> createState() => _PictureViewerState();
}

class _PictureViewerState extends State<PictureViewer> {
  late final PageController _pages = PageController(initialPage: widget.initial);
  late int _index = widget.initial;
  bool _zoomed = false;

  @override
  void dispose() {
    _pages.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final int count = widget.images.length;
    return Scaffold(
      backgroundColor: Kit.dark.sidebar,
      appBar: AppBar(
        backgroundColor: Kit.dark.sidebar,
        leading: IconButton(tooltip: l10n.pictureClose, icon: const Icon(Icons.close), onPressed: () => Navigator.of(context).pop()),
        title: count > 1 ? Text('${_index + 1} / $count', style: Kit.of(context).label) : null,
        centerTitle: true,
      ),
      body: PageView.builder(
        controller: _pages,
        physics: _zoomed ? const NeverScrollableScrollPhysics() : const PageScrollPhysics(),
        itemCount: count,
        onPageChanged: (int index) => setState(() {
          _index = index;
          _zoomed = false;
        }),
        itemBuilder: (BuildContext context, int index) => _PicturePage(
          key: ValueKey<String>(widget.images[index].id),
          sessionId: widget.sessionId,
          image: widget.images[index],
          preview: index == widget.initial ? widget.preview : null,
          onZoom: (bool zoomed) {
            if (zoomed != _zoomed) setState(() => _zoomed = zoomed);
          },
        ),
      ),
    );
  }
}

/// One picture in the viewer: thumbnail first (from memory or disk, usually already loaded), then the large one.
class _PicturePage extends ConsumerStatefulWidget {
  const _PicturePage({super.key, required this.sessionId, required this.image, required this.preview, required this.onZoom});
  final String sessionId;
  final ChatImage image;
  final Uint8List? preview;
  final ValueChanged<bool> onZoom;

  @override
  ConsumerState<_PicturePage> createState() => _PicturePageState();
}

class _PicturePageState extends ConsumerState<_PicturePage> {
  late Uint8List? _shown = widget.preview;
  bool _sharp = false;
  bool _failed = false;
  final TransformationController _transform = TransformationController();

  @override
  void initState() {
    super.initState();
    _transform.addListener(_reportZoom);
    unawaited(_load());
  }

  @override
  void dispose() {
    _transform
      ..removeListener(_reportZoom)
      ..dispose();
    super.dispose();
  }

  void _reportZoom() => widget.onZoom(_transform.value.getMaxScaleOnAxis() > 1.01);

  Future<void> _load() async {
    final ChatPictureLoader loader = ref.read(chatPictureLoaderProvider);
    if (_shown == null) {
      try {
        final Uint8List thumb = await loader.load(widget.sessionId, widget.image);
        if (mounted && !_sharp) setState(() => _shown = thumb);
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
        ? InteractiveViewer(
            transformationController: _transform,
            minScale: 1,
            maxScale: 6,
            child: Center(child: Image.memory(shown, fit: BoxFit.contain, gaplessPlayback: true)),
          )
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
