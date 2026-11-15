import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/tokens.dart';
import '../../app/widgets/common.dart';
import '../../app/widgets/kit.dart';
import '../../core/net/bridge_client.dart';
import '../../core/protocol/generated.dart' as wire;
import '../../l10n/app_localizations.dart';
import '../connection/connection.dart';
import '../macs/macs_notifier.dart';
import 'held_notifier.dart';

class HeldSection extends ConsumerWidget {
  const HeldSection({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final String? active = ref.watch(macsProvider.select((MacsState s) => s.active?.id));
    if (active == null) return const SizedBox.shrink();
    final HeldState held = ref.watch(computerHeldProvider(active));
    final ConnectionStatus status = ref.watch(connectionStatusProvider);
    final String own = ref.watch(macsProvider.select((MacsState s) => s.active?.name ?? ''));
    final ComputerHeldNotifier notifier = ref.read(computerHeldProvider(active).notifier);
    return HeldList(
      entries: held.entries,
      notices: held.notices,
      nodeName: held.holder.isEmpty ? (status.via ?? own) : held.holder,
      primaryAnswers: status.primaryAnswers,
      enabled: status.phase == LinkPhase.connected,
      failed: held.failed,
      onCancel: (String id) => unawaited(notifier.cancel(id)),
      onConfirm: (String id) => unawaited(notifier.confirm(id)),
      onDismiss: notifier.dismiss,
    );
  }
}

class HeldList extends StatelessWidget {
  const HeldList({
    super.key,
    required this.entries,
    required this.notices,
    required this.nodeName,
    required this.primaryAnswers,
    required this.onCancel,
    required this.onConfirm,
    required this.onDismiss,
    this.enabled = true,
    this.failed = false,
  });

  final bool enabled;
  final bool failed;
  final List<wire.HeldEntry> entries;
  final List<wire.HeldNotice> notices;
  final String nodeName;
  final bool primaryAnswers;
  final void Function(String id) onCancel;
  final void Function(String id) onConfirm;
  final void Function(String id) onDismiss;

  @override
  Widget build(BuildContext context) {
    if (entries.isEmpty && notices.isEmpty) return const SizedBox.shrink();
    final AppL10n l10n = AppL10n.of(context);
    final Kit kit = Kit.of(context);
    return Padding(
      padding: const EdgeInsets.fromLTRB(Space.lg, Space.xs, Space.lg, Space.sm),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: <Widget>[
          for (final wire.HeldNotice notice in notices)
            Padding(
              padding: const EdgeInsets.only(bottom: Space.sm),
              child: Surface(
                padding: const EdgeInsets.all(Space.md),
                child: Row(
                  children: <Widget>[
                    Icon(Icons.info_outline, size: 16, color: kit.textMuted),
                    const SizedBox(width: Space.sm),
                    Expanded(child: Text(notice.reason, style: kit.body)),
                    TextButton(onPressed: () => onDismiss(notice.id), child: Text(l10n.sessionsHeldDismiss)),
                  ],
                ),
              ),
            ),
          for (final wire.HeldEntry entry in entries)
            Padding(
              padding: const EdgeInsets.only(bottom: Space.sm),
              child: _HeldRow(
                entry: entry,
                nodeName: nodeName,
                primaryAnswers: primaryAnswers,
                onCancel: enabled ? () => onCancel(entry.id) : null,
                onConfirm: enabled ? () => onConfirm(entry.id) : null,
              ),
            ),
          if (failed && entries.isNotEmpty)
            Padding(
              padding: const EdgeInsets.only(bottom: Space.sm),
              child: Text(l10n.sessionsHeldFailed, style: kit.meta.copyWith(color: kit.danger)),
            ),
        ],
      ),
    );
  }
}

class _HeldRow extends StatelessWidget {
  const _HeldRow({required this.entry, required this.nodeName, required this.primaryAnswers, required this.onCancel, required this.onConfirm});

  final wire.HeldEntry entry;
  final String nodeName;
  final bool primaryAnswers;
  final VoidCallback? onCancel;
  final VoidCallback? onConfirm;

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final Kit kit = Kit.of(context);
    final bool asking = entry.waiting && primaryAnswers;
    final String heldOn = l10n.sessionsHeldOn(nodeName);
    return Semantics(
      container: true,
      label: <String>[if (entry.waiting) l10n.sessionsHeldWaiting, heldOn, entry.summary].join(' '),
      child: Surface(
        padding: const EdgeInsets.all(Space.md),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: <Widget>[
            ExcludeSemantics(
              child: Row(
                children: <Widget>[
                  Icon(entry.waiting ? Icons.hourglass_empty : Icons.schedule_send_outlined, size: 14, color: kit.textMuted),
                  const SizedBox(width: Space.xs),
                  if (entry.waiting) ...<Widget>[
                    Text(l10n.sessionsHeldWaiting, style: kit.meta.copyWith(color: kit.needs)),
                    const SizedBox(width: Space.sm),
                  ],
                  Flexible(child: Text(heldOn, style: kit.meta)),
                ],
              ),
            ),
            const SizedBox(height: Space.xs),
            Text(entry.summary, style: kit.body, maxLines: 3, overflow: TextOverflow.ellipsis),
            if (asking) ...<Widget>[
              const SizedBox(height: Space.sm),
              Text(l10n.sessionsHeldStillSend, style: kit.heading),
            ],
            const SizedBox(height: Space.sm),
            Wrap(
              spacing: Space.sm,
              runSpacing: Space.sm,
              children: <Widget>[
                if (asking) PrimaryButton(label: l10n.sessionsHeldSend, onPressed: onConfirm),
                SecondaryButton(label: l10n.sessionsHeldCancel, onPressed: onCancel),
              ],
            ),
          ],
        ),
      ),
    );
  }
}
