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
    final HeldState held = ref.watch(heldProvider);
    final ConnectionStatus status = ref.watch(connectionStatusProvider);
    final String own = ref.watch(macsProvider.select((MacsState s) => s.active?.name ?? ''));
    final HeldNotifier notifier = ref.read(heldProvider.notifier);
    return HeldList(
      entries: held.entries,
      notices: held.notices,
      nodeName: status.via ?? own,
      primaryAnswers: status.primaryAnswers,
      onCancel: (String id) => notifier.cancel(id),
      onConfirm: (String id) => notifier.confirm(id),
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
  });

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
                onCancel: () => onCancel(entry.id),
                onConfirm: () => onConfirm(entry.id),
              ),
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
  final VoidCallback onCancel;
  final VoidCallback onConfirm;

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final Kit kit = Kit.of(context);
    final bool asking = entry.waiting && primaryAnswers;
    final String heldOn = l10n.sessionsHeldOn(nodeName);
    return Semantics(
      container: true,
      label: entry.waiting ? '${l10n.sessionsHeldWaiting}. $heldOn. ${entry.summary}' : '$heldOn. ${entry.summary}',
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
