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
import '../macs/computers.dart';
import 'chat_cache.dart';

const int thumbnailWidth = 480;
const int viewerWidth = 1600;

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
  final ChatCache cache = ref.watch(scopedChatCacheProvider);
  return ChatPictureLoader(cache, () => ref.read(bridgeClientProvider));
});

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
        children: <Widget>[for (final ChatImage image in images) PictureThumbnail(key: ValueKey<String>(image.id), sessionId: sessionId, image: image)],
      ),
    );
  }
}

enum PictureLoad { loading, ready, failed }

class PictureThumbnail extends ConsumerStatefulWidget {
  const PictureThumbnail({super.key, required this.sessionId, required this.image});
  final String sessionId;
  final ChatImage image;

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
                builder: (_) => PictureViewer(sessionId: widget.sessionId, image: widget.image, preview: _bytes!),
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

class PictureViewer extends ConsumerStatefulWidget {
  const PictureViewer({super.key, required this.sessionId, required this.image, required this.preview});
  final String sessionId;
  final ChatImage image;
  final Uint8List preview;

  @override
  ConsumerState<PictureViewer> createState() => _PictureViewerState();
}

class _PictureViewerState extends ConsumerState<PictureViewer> {
  late Uint8List _shown = widget.preview;
  bool _sharp = false;

  @override
  void initState() {
    super.initState();
    unawaited(_loadLarge());
  }

  Future<void> _loadLarge() async {
    try {
      final Uint8List bytes = await ref.read(chatPictureLoaderProvider).load(widget.sessionId, widget.image, width: viewerWidth);
      if (mounted) {
        setState(() {
          _shown = bytes;
          _sharp = true;
        });
      }
    } on BridgeException catch (error) {
      debugPrint('PictureViewer(${widget.image.id}): ${error.code} ${error.message}');
    }
  }

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    return Scaffold(
      backgroundColor: Kit.dark.sidebar,
      appBar: AppBar(
        backgroundColor: Kit.dark.sidebar,
        leading: IconButton(tooltip: l10n.pictureClose, icon: const Icon(Icons.close), onPressed: () => Navigator.of(context).pop()),
        bottom: _sharp ? null : const PreferredSize(preferredSize: Size.fromHeight(2), child: LinearProgressIndicator(minHeight: 2)),
      ),
      body: InteractiveViewer(
        minScale: 1,
        maxScale: 6,
        child: Center(child: Image.memory(_shown, fit: BoxFit.contain, gaplessPlayback: true)),
      ),
    );
  }
}
