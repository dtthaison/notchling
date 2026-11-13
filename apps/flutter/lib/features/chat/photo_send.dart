import 'dart:async';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:image_picker/image_picker.dart';

import '../../app/theme/tokens.dart';
import '../../app/widgets/common.dart';
import '../../core/net/bridge_client.dart';
import '../../l10n/app_localizations.dart';
import '../connection/connection.dart';

enum PhotoOrigin { camera, gallery }

abstract interface class PhotoPicker {
  Future<Uint8List?> pick(PhotoOrigin origin);
}

class SystemPhotoPicker implements PhotoPicker {
  final ImagePicker _picker = ImagePicker();

  @override
  Future<Uint8List?> pick(PhotoOrigin origin) async {
    final XFile? file = await _picker.pickImage(
      source: origin == PhotoOrigin.camera ? ImageSource.camera : ImageSource.gallery,
      maxWidth: 1600,
      maxHeight: 1600,
      imageQuality: 82,
    );
    return file?.readAsBytes();
  }
}

final Provider<PhotoPicker> photoPickerProvider = Provider<PhotoPicker>((Ref ref) => SystemPhotoPicker());

Future<void> pickAndSendPhoto(BuildContext context, WidgetRef ref, String sessionId) async {
  final AppL10n l10n = AppL10n.of(context);
  final PhotoOrigin? origin = await showModalBottomSheet<PhotoOrigin>(
    context: context,
    showDragHandle: true,
    builder: (BuildContext sheet) {
      final Kit kit = Kit.of(sheet);
      return SafeArea(
        child: Column(mainAxisSize: MainAxisSize.min, children: <Widget>[
          ListTile(
            leading: Icon(Icons.photo_camera_outlined, color: kit.textMuted),
            title: Text(l10n.photoCamera, style: kit.body),
            minVerticalPadding: Space.md,
            onTap: () => Navigator.of(sheet).pop(PhotoOrigin.camera),
          ),
          ListTile(
            leading: Icon(Icons.photo_library_outlined, color: kit.textMuted),
            title: Text(l10n.photoGallery, style: kit.body),
            minVerticalPadding: Space.md,
            onTap: () => Navigator.of(sheet).pop(PhotoOrigin.gallery),
          ),
        ]),
      );
    },
  );
  if (origin == null || !context.mounted) return;
  final ScaffoldMessengerState messenger = ScaffoldMessenger.of(context);
  final Uint8List? bytes;
  try {
    bytes = await ref.read(photoPickerProvider).pick(origin);
  } on Exception catch (error) {
    debugPrint('pickAndSendPhoto($sessionId, $origin): $error');
    messenger.showSnackBar(SnackBar(content: Text(l10n.photoPickFailed)));
    return;
  }
  if (bytes == null || !context.mounted) return;
  final bool sent = await Navigator.of(context).push<bool>(
        MaterialPageRoute<bool>(
          fullscreenDialog: true,
          builder: (BuildContext route) => PhotoPreview(
            bytes: bytes!,
            send: (String caption, void Function(double) onProgress) async {
              final BridgeClient? client = ref.read(bridgeClientProvider);
              if (client == null || !client.isConnected) throw const BridgeException('disconnected', 'Chưa kết nối tới Mac.');
              await client.sessionImage(sessionId, bytes!, caption: caption, onProgress: onProgress);
            },
          ),
        ),
      ) ??
      false;
  if (sent) messenger.showSnackBar(SnackBar(content: Text(l10n.photoSent)));
}

class PhotoPreview extends StatefulWidget {
  const PhotoPreview({super.key, required this.bytes, required this.send});
  final Uint8List bytes;
  final Future<void> Function(String caption, void Function(double fraction) onProgress) send;

  @override
  State<PhotoPreview> createState() => _PhotoPreviewState();
}

class _PhotoPreviewState extends State<PhotoPreview> {
  final TextEditingController _caption = TextEditingController();
  double? _progress;
  String? _error;

  @override
  void dispose() {
    _caption.dispose();
    super.dispose();
  }

  Future<void> _send() async {
    setState(() {
      _progress = 0;
      _error = null;
    });
    try {
      await widget.send(_caption.text, (double fraction) {
        if (mounted) setState(() => _progress = fraction);
      });
      if (mounted) Navigator.of(context).pop(true);
    } on BridgeException catch (error) {
      debugPrint('PhotoPreview.send: ${error.code} ${error.message}');
      if (mounted) {
        setState(() {
          _progress = null;
          _error = error.message;
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final Kit kit = Kit.of(context);
    final bool sending = _progress != null;
    return Scaffold(
      appBar: AppBar(
        backgroundColor: kit.sidebar,
        leading: IconButton(tooltip: l10n.close, icon: const Icon(Icons.close), onPressed: sending ? null : () => Navigator.of(context).pop(false)),
        title: Text(l10n.photoTitle, style: kit.heading),
      ),
      body: SafeArea(
        child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: <Widget>[
          Expanded(
            child: Padding(
              padding: const EdgeInsets.all(Space.lg),
              child: ClipRRect(borderRadius: BorderRadius.circular(Radii.card), child: Image.memory(widget.bytes, fit: BoxFit.contain, gaplessPlayback: true)),
            ),
          ),
          if (sending) LinearProgressIndicator(value: _progress, minHeight: 2),
          Padding(
            padding: const EdgeInsets.fromLTRB(Space.lg, Space.md, Space.lg, Space.lg),
            child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: <Widget>[
              if (_error != null)
                Padding(
                  padding: const EdgeInsets.only(bottom: Space.sm),
                  child: Text(_error!, style: kit.meta.copyWith(color: kit.danger)),
                ),
              TextField(
                controller: _caption,
                enabled: !sending,
                minLines: 1,
                maxLines: 4,
                textInputAction: TextInputAction.newline,
                style: kit.body,
                decoration: InputDecoration(hintText: l10n.photoCaptionHint),
              ),
              const SizedBox(height: Space.md),
              PrimaryButton(label: sending ? l10n.sending : l10n.send, icon: sending ? null : Icons.arrow_upward, onPressed: sending ? null : () => unawaited(_send())),
            ]),
          ),
        ]),
      ),
    );
  }
}
