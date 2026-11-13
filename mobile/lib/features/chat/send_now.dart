import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/tokens.dart';
import '../../core/net/bridge_client.dart';
import '../../core/protocol/models.dart';
import '../../l10n/app_localizations.dart';
import '../connection/connection.dart';
import '../sessions/sessions_notifier.dart';

/// False once a Mac has answered `session.sendNow` with "no such op" (an older Notchling): the action is hidden
/// from then on instead of failing on every queued message.
class SendNowSupport extends Notifier<bool> {
  @override
  bool build() => true;

  void markUnsupported() => state = false;
}

final NotifierProvider<SendNowSupport, bool> sendNowSupportProvider = NotifierProvider<SendNowSupport, bool>(SendNowSupport.new);

/// The "Gửi ngay" action under a queued bubble: confirms, asks the Mac to interrupt the running turn, and shows a
/// spinner until the transcript turns the bubble into a normal one (the widget is then gone).
class SendNowAction extends ConsumerStatefulWidget {
  const SendNowAction({super.key, required this.sessionId});
  final String sessionId;

  @override
  ConsumerState<SendNowAction> createState() => _SendNowActionState();
}

class _SendNowActionState extends ConsumerState<SendNowAction> {
  bool _busy = false;

  Future<void> _run() async {
    final AppL10n l10n = AppL10n.of(context);
    final bool? confirmed = await showDialog<bool>(
      context: context,
      builder: (BuildContext context) => AlertDialog(
        title: Text(l10n.chatSendNowTitle),
        content: Text(l10n.chatSendNowBody),
        actions: <Widget>[
          TextButton(onPressed: () => Navigator.pop(context, false), child: Text(l10n.cancel)),
          TextButton(onPressed: () => Navigator.pop(context, true), child: Text(l10n.chatSendNow)),
        ],
      ),
    );
    final BridgeClient? client = ref.read(bridgeClientProvider);
    if (confirmed != true || !mounted || client == null) return;
    setState(() => _busy = true);
    final ScaffoldMessengerState messenger = ScaffoldMessenger.of(context);
    try {
      await client.sessionSendNow(widget.sessionId);
      // Stays busy: the queued bubble is replaced by the transcript within a second or two.
      unawaited(Future<void>.delayed(const Duration(seconds: 8), () {
        if (mounted) setState(() => _busy = false);
      }));
    } on BridgeException catch (error) {
      if (error.isInvalid && error.message.contains('lệnh')) ref.read(sendNowSupportProvider.notifier).markUnsupported();
      messenger.showSnackBar(SnackBar(content: Text('${l10n.chatSendNowFailed}: ${error.message}')));
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final SessionControl? control = ref.watch(sessionsProvider.select((SessionsState s) => s.byId(widget.sessionId)?.control));
    final bool supported = ref.watch(sendNowSupportProvider);
    if (!supported || control != SessionControl.console) return const SizedBox.shrink();
    final Kit kit = Kit.of(context);
    final AppL10n l10n = AppL10n.of(context);
    return Padding(
      padding: const EdgeInsets.only(top: Space.xs),
      child: InkWell(
        borderRadius: BorderRadius.circular(Radii.full),
        onTap: _busy ? null : () => unawaited(_run()),
        child: ConstrainedBox(
          constraints: const BoxConstraints(minHeight: Metric.minTarget),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: Space.sm),
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: <Widget>[
                if (_busy)
                  const SizedBox(width: 14, height: 14, child: CircularProgressIndicator(strokeWidth: 2))
                else
                  Icon(Icons.bolt, size: 16, color: kit.accent),
                const SizedBox(width: Space.xs),
                Text(l10n.chatSendNow, style: kit.meta.copyWith(color: kit.accent, fontWeight: FontWeight.w600)),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
