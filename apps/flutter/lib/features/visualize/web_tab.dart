import 'dart:async';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/tokens.dart';
import '../../app/widgets/common.dart';
import '../../core/net/bridge_client.dart';
import '../../l10n/app_localizations.dart';
import '../connection/connection.dart';

class WebTab extends ConsumerStatefulWidget {
  const WebTab({super.key, required this.sessionId, this.initialUrl, this.candidates = const <String>[]});
  final String sessionId;
  final String? initialUrl;

  final List<String> candidates;

  @override
  ConsumerState<WebTab> createState() => _WebTabState();
}

class _WebTabState extends ConsumerState<WebTab> {
  late final TextEditingController _url = TextEditingController(text: widget.initialUrl ?? '');
  Uint8List? _image;
  bool _loading = false;
  bool _failed = false;
  String? _reason;

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
      _reason = null;
    });
    try {
      final Uint8List jpeg = await client.webSnapshot(widget.sessionId, url: url.isEmpty ? null : url, width: 600);
      if (mounted) setState(() => _image = jpeg);
    } on BridgeException catch (error) {
      if (mounted) {
        setState(() {
          _failed = true;
          _reason = error.code == 'not_reachable' || error.code == 'invalid' || error.code == 'not_found' ? error.message : null;
        });
      }
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
      content = Column(mainAxisAlignment: MainAxisAlignment.center, children: <Widget>[
        ErrorState(message: _reason ?? l10n.visualizeWebFailed, onRetry: () => unawaited(_capture())),
        if (_reason != null)
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: Space.xl),
            child: Text(l10n.visualizeWebHint, textAlign: TextAlign.center, style: Kit.of(context).meta),
          ),
      ]);
    } else if (image != null) {
      content = InteractiveViewer(maxScale: 4, child: Center(child: Image.memory(image, gaplessPlayback: true)));
    } else {
      content = StateMessage(icon: Icons.public, title: l10n.visualizeWebNoUrl);
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
              style: Kit.of(context).codeSm.copyWith(color: Kit.of(context).text),
              decoration: InputDecoration(hintText: 'http://localhost:3000', labelText: l10n.visualizeWebUrl),
              onSubmitted: (_) => unawaited(_capture()),
            ),
          ),
          const SizedBox(width: Space.sm),
          Semantics(
            button: true,
            label: l10n.visualizeWebRefresh,
            excludeSemantics: true,
            child: IconButton(
              onPressed: _loading ? null : () => unawaited(_capture()),
              icon: const Icon(Icons.refresh),
              tooltip: l10n.visualizeWebRefresh,
              constraints: const BoxConstraints(minWidth: Metric.minTarget, minHeight: Metric.minTarget),
            ),
          ),
        ]),
      ),
      if (widget.candidates.isNotEmpty)
        SizedBox(
          height: Metric.minTarget,
          child: ListView(
            scrollDirection: Axis.horizontal,
            padding: const EdgeInsets.symmetric(horizontal: Space.md),
            children: <Widget>[
              for (final String candidate in widget.candidates)
                Padding(
                  padding: const EdgeInsets.only(right: Space.sm),
                  child: ActionChip(
                    label: Text(candidate.replaceFirst('http://', ''), style: Kit.of(context).codeSm),
                    onPressed: () {
                      _url.text = candidate;
                      unawaited(_capture());
                    },
                  ),
                ),
            ],
          ),
        ),
      Expanded(child: content),
    ]);
  }
}
