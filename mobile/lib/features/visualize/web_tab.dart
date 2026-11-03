import 'dart:async';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/tokens.dart';
import '../../app/widgets/common.dart';
import '../../core/net/bridge_client.dart';
import '../../l10n/app_localizations.dart';
import '../connection/connection.dart';

/// A picture of the session's dev server (v1 is a snapshot, not an interactive page).
class WebTab extends ConsumerStatefulWidget {
  const WebTab({super.key, required this.sessionId, this.initialUrl});
  final String sessionId;
  final String? initialUrl;

  @override
  ConsumerState<WebTab> createState() => _WebTabState();
}

class _WebTabState extends ConsumerState<WebTab> {
  late final TextEditingController _url = TextEditingController(text: widget.initialUrl ?? '');
  Uint8List? _image;
  bool _loading = false;
  bool _failed = false;

  @override
  void initState() {
    super.initState();
    if ((widget.initialUrl ?? '').isNotEmpty) unawaited(_capture());
  }

  @override
  void dispose() {
    _url.dispose();
    super.dispose();
  }

  Future<void> _capture() async {
    final BridgeClient? client = ref.read(bridgeClientProvider);
    if (client == null) return;
    final String url = _url.text.trim();
    setState(() {
      _loading = true;
      _failed = false;
    });
    try {
      final Uint8List jpeg = await client.webSnapshot(widget.sessionId, url: url.isEmpty ? null : url, width: 600);
      if (mounted) setState(() => _image = jpeg);
    } on BridgeException {
      if (mounted) setState(() => _failed = true);
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final Uint8List? image = _image;
    Widget content;
    if (_loading && image == null) {
      content = const SkeletonRows(count: 4);
    } else if (_failed && image == null) {
      content = ErrorState(message: l10n.vizWebFailed, onRetry: () => unawaited(_capture()));
    } else if (image != null) {
      content = InteractiveViewer(maxScale: 4, child: Center(child: Image.memory(image, gaplessPlayback: true)));
    } else {
      content = StateMessage(icon: Icons.public, title: l10n.vizWebNoUrl);
    }
    return Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: <Widget>[
      Padding(
        padding: const EdgeInsets.all(Space.md),
        child: Row(children: <Widget>[
          Expanded(
            child: TextField(
              controller: _url,
              keyboardType: TextInputType.url,
              autocorrect: false,
              style: TypeToken.codeSm.copyWith(color: Token.onSurface),
              decoration: InputDecoration(hintText: 'http://localhost:3000', labelText: l10n.vizWebUrl),
              onSubmitted: (_) => unawaited(_capture()),
            ),
          ),
          const SizedBox(width: Space.sm),
          Semantics(
            button: true,
            label: l10n.vizWebRefresh,
            excludeSemantics: true,
            child: IconButton(
              onPressed: _loading ? null : () => unawaited(_capture()),
              icon: const Icon(Icons.refresh),
              tooltip: l10n.vizWebRefresh,
              constraints: const BoxConstraints(minWidth: Metric.minTarget, minHeight: Metric.minTarget),
            ),
          ),
        ]),
      ),
      Expanded(child: content),
    ]);
  }
}
