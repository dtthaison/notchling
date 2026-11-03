import 'dart:async';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/tokens.dart';
import '../../app/widgets/common.dart';
import '../../core/net/bridge_client.dart';
import '../../l10n/app_localizations.dart';
import '../connection/connection.dart';
import '../connection/subscription_sync.dart';

/// The simulator or emulator window, streamed by the Mac as JPEG frames while this tab is open.
class PhoneTab extends ConsumerStatefulWidget {
  const PhoneTab({super.key});

  @override
  ConsumerState<PhoneTab> createState() => _PhoneTabState();
}

class _PhoneTabState extends ConsumerState<PhoneTab> {
  StreamSubscription<PhoneFrame>? _frames;
  Uint8List? _latest;
  late final PhoneWantedNotifier _wanted = ref.read(phoneWantedProvider.notifier);

  @override
  void initState() {
    super.initState();
    Future<void>.microtask(() => _wanted.set(true));
    _bind(ref.read(bridgeClientProvider));
  }

  void _bind(BridgeClient? client) {
    unawaited(_frames?.cancel());
    _frames = client?.phoneFrames.listen((PhoneFrame frame) {
      if (mounted) setState(() => _latest = frame.jpeg);
    });
  }

  @override
  void dispose() {
    unawaited(_frames?.cancel());
    // Provider state may not change while the tree is being torn down.
    Future<void>.microtask(() => _wanted.set(false));
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    ref.listen<BridgeClient?>(bridgeClientProvider, (BridgeClient? previous, BridgeClient? next) => _bind(next));
    final String? error = ref.watch(phoneErrorProvider);
    final Uint8List? frame = _latest;

    if (frame != null && error == null) {
      return Semantics(
        image: true,
        label: 'Phone',
        child: Container(
          color: Token.neutral,
          alignment: Alignment.center,
          padding: const EdgeInsets.all(Space.sm),
          child: Image.memory(frame, gaplessPlayback: true, fit: BoxFit.contain, filterQuality: FilterQuality.low),
        ),
      );
    }
    if (error != null) {
      final String lower = error.toLowerCase();
      final bool permission = lower.contains('quyền') || lower.contains('permission');
      final bool noWindow = lower.contains('cửa sổ') || lower.contains('window');
      return StateMessage(
        icon: permission ? Icons.lock_outline : (noWindow ? Icons.phonelink_off : Icons.error_outline),
        title: permission ? l10n.vizPhoneNoPermission : (noWindow ? l10n.vizPhoneNoWindow : l10n.vizPhoneError(error)),
        body: permission || noWindow ? error : null,
        isError: !permission && !noWindow,
      );
    }
    return StateMessage(icon: Icons.phone_iphone, title: l10n.vizPhoneStarting);
  }
}
