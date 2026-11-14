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
import '../macs/session_ref.dart';
import 'sessions_notifier.dart';

Future<void> runTakeover(
  BuildContext context,
  WidgetRef ref,
  Session session,
) async {
  final AppL10n l10n = AppL10n.of(context);
  final ScaffoldMessengerState messenger = ScaffoldMessenger.of(context);
  final TakeoverChoice choice = await confirmTakeover(context, session);
  if (choice == TakeoverChoice.cancel || !context.mounted) return;
  final SessionRef? on = SessionRef.parse(session.id);
  final BridgeClient? client = on == null
      ? ref.read(bridgeClientProvider)
      : ref.read(computerClientProvider(on.host));
  if (client == null) return;
  try {
    final String newId = await client.sessionTakeover(
      on?.id ?? session.id,
      stopOriginal: choice == TakeoverChoice.stopAndResume,
    );
    if (on == null) {
      ref.read(selectedSessionIdProvider.notifier).select(newId);
    } else {
      ref.read(selectedSessionProvider.notifier).select(SessionRef(on.host, newId));
    }
    ref.read(activeTabProvider.notifier).show(1);
  } on BridgeException catch (error) {
    messenger.showSnackBar(
      SnackBar(content: Text(l10n.appTakeoverFailed(error.message))),
    );
  }
}

class TakeoverRowButton extends ConsumerStatefulWidget {
  const TakeoverRowButton({super.key, required this.session});
  final Session session;

  @override
  ConsumerState<TakeoverRowButton> createState() => _TakeoverRowButtonState();
}

class _TakeoverRowButtonState extends ConsumerState<TakeoverRowButton> {
  bool _working = false;

  Future<void> _run() async {
    setState(() => _working = true);
    try {
      await runTakeover(context, ref, widget.session);
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
      label: '${l10n.appTakeover}, ${widget.session.name}',
      excludeSemantics: true,
      onTap: _working ? null : () => unawaited(_run()),
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: _working ? null : () => unawaited(_run()),
        child: ConstrainedBox(
          constraints: const BoxConstraints(
            minWidth: Metric.minTarget,
            minHeight: Metric.minTarget,
          ),
          child: Center(
            child: Container(
              padding: const EdgeInsets.symmetric(
                horizontal: Space.md,
                vertical: Space.sm,
              ),
              decoration: BoxDecoration(
                color: Kit.of(context).sidebar,
                borderRadius: BorderRadius.circular(Radii.md),
                border: Border.all(color: Kit.of(context).hairline),
              ),
              child: _working
                  ? const SizedBox(
                      width: 14,
                      height: 14,
                      child: CircularProgressIndicator(strokeWidth: 2),
                    )
                  : Text(
                      l10n.appTakeover,
                      style: Kit.of(
                        context,
                      ).labelSm.copyWith(color: Kit.of(context).text),
                    ),
            ),
          ),
        ),
      ),
    );
  }
}
