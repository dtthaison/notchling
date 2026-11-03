import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:webview_flutter/webview_flutter.dart';

import '../../app/theme/tokens.dart';
import '../../app/widgets/common.dart';
import '../../core/protocol/json.dart';
import '../../l10n/app_localizations.dart';

const String vizAssetPath = 'assets/viz/index.html';

/// Safe JavaScript call for a spec: the JSON text becomes a JS string literal (via a second jsonEncode),
/// which `JSON.parse` reads back, so no spec text is ever spliced into the script itself.
String renderScript(Json spec) => 'window.renderViz(JSON.parse(${jsonEncode(jsonEncode(spec))}));';

/// The shared viz bundle (../viz, copied by tool/sync_viz.sh) in a WebView, loaded from Flutter assets.
/// It never leaves the bundle: navigation to anything but local files is refused.
class VizWebView extends StatefulWidget {
  const VizWebView({super.key, required this.spec});
  final Json spec;

  @override
  State<VizWebView> createState() => _VizWebViewState();
}

class _VizWebViewState extends State<VizWebView> {
  late final WebViewController _controller;
  bool _ready = false;
  bool? _bundlePresent;

  @override
  void initState() {
    super.initState();
    unawaited(_checkBundle());
    _controller = WebViewController()
      ..setJavaScriptMode(JavaScriptMode.unrestricted)
      ..setBackgroundColor(Token.surface)
      ..setNavigationDelegate(NavigationDelegate(
        onPageFinished: (String url) {
          _ready = true;
          _render();
        },
        onNavigationRequest: (NavigationRequest request) =>
            request.url.startsWith('file:') || request.url == 'about:blank' ? NavigationDecision.navigate : NavigationDecision.prevent,
      ))
      ..loadFlutterAsset(vizAssetPath);
  }

  Future<void> _checkBundle() async {
    try {
      await rootBundle.load(vizAssetPath);
      if (mounted) setState(() => _bundlePresent = true);
    } on FlutterError {
      if (mounted) setState(() => _bundlePresent = false);
    }
  }

  @override
  void didUpdateWidget(VizWebView oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.spec != widget.spec) _render();
  }

  void _render() {
    if (_ready) unawaited(_controller.runJavaScript(renderScript(widget.spec)));
  }

  @override
  Widget build(BuildContext context) {
    if (_bundlePresent == false) {
      return StateMessage(icon: Icons.extension_off, title: AppL10n.of(context).vizBundleMissing, isError: true);
    }
    return WebViewWidget(controller: _controller);
  }
}
