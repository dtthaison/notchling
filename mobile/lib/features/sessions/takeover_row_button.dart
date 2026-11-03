import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/nav_state.dart';
import '../../app/theme/tokens.dart';
import '../../core/net/bridge_client.dart';
import '../../core/protocol/models.dart';
import '../../l10n/app_localizations.dart';
import '../chat/takeover_flow.dart';
import '../connection/connection.dart';
import 'sessions_notifier.dart';

/// "Tiếp quản" on a view-only session's row: one tap (and the confirmation) from the list, no need to open the
/// chat first. On success the chat of the resumed session opens.
class TakeoverRowButton extends ConsumerStatefulWidget {
  const TakeoverRowButton({super.key, required this.session});
  final Session session;

  @override
  ConsumerState<TakeoverRowButton> createState() => _TakeoverRowButtonState();
}

class _TakeoverRowButtonState extends ConsumerState<TakeoverRowButton> {
  bool _working = false;

  Future<void> _run() async {
    final AppL10n l10n = AppL10n.of(context);
    final ScaffoldMessengerState messenger = ScaffoldMessenger.of(context);
    final TakeoverChoice choice = await confirmTakeover(context, widget.session);
    if (choice == TakeoverChoice.cancel || !mounted) return;
    final BridgeClient? client = ref.read(bridgeClientProvider);
    if (client == null) return;
    setState(() => _working = true);
    try {
      final String newId = await client.sessionTakeover(widget.session.id, stopOriginal: choice == TakeoverChoice.stopAndResume);
      if (!mounted) return;
      ref.read(selectedSessionIdProvider.notifier).select(newId);
      ref.read(activeTabProvider.notifier).show(1);
    } on BridgeException catch (error) {
      messenger.showSnackBar(SnackBar(content: Text(l10n.takeoverFailed(error.message))));
    } finally {
      if (mounted) setState(() => _working = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    return Semantics(
      button: true,
      enabled: !_working,
      label: '${l10n.takeover}, ${widget.session.name}',
      excludeSemantics: true,
      onTap: _working ? null : () => unawaited(_run()),
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: _working ? null : () => unawaited(_run()),
        child: ConstrainedBox(
          constraints: const BoxConstraints(minWidth: Metric.minTarget, minHeight: Metric.minTarget),
          child: Center(
            child: Container(
              padding: const EdgeInsets.symmetric(horizontal: Space.md, vertical: Space.sm),
              decoration: BoxDecoration(
                color: Token.surface,
                borderRadius: BorderRadius.circular(Radii.md),
                border: Border.all(color: Token.border),
              ),
              child: _working
                  ? const SizedBox(width: 14, height: 14, child: CircularProgressIndicator(strokeWidth: 2))
                  : Text(l10n.takeover, style: TypeToken.labelSm.copyWith(color: Token.onSurface)),
            ),
          ),
        ),
      ),
    );
  }
}
