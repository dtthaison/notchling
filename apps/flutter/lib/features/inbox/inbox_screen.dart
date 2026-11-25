import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/tokens.dart';
import '../../app/widgets/common.dart';
import '../../app/widgets/kit.dart';
import '../../core/protocol/models.dart';
import '../../l10n/app_localizations.dart';
import '../../core/net/bridge_client.dart';
import '../connection/bridge_errors.dart';
import '../connection/connection.dart';
import '../machines/machines_notifier.dart';
import '../machines/machines_sheet.dart';
import 'inbox_notifier.dart';
import 'promote_confirm.dart';

class InboxScreen extends ConsumerWidget {
  const InboxScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final AppL10n l10n = AppL10n.of(context);
    final InboxState state = ref.watch(inboxProvider);
    if (state.phase == InboxPhase.loading) return const SkeletonRows();
    if (state.phase == InboxPhase.unsupported) {
      return StateMessage(icon: Icons.inbox_outlined, title: l10n.inboxUnsupported);
    }
    if (state.items.isEmpty) {
      return StateMessage(
        icon: Icons.inbox_outlined,
        title: l10n.inboxEmptyTitle,
        body: l10n.inboxEmptyBody,
      );
    }
    final List<InboxItem> needsYou = state.needsYou;
    final List<InboxItem> done = state.done;
    return RefreshIndicator(
      onRefresh: () => ref.read(inboxProvider.notifier).refresh(),
      child: ListView(
        padding: const EdgeInsets.fromLTRB(Space.lg, Space.lg, Space.lg, Space.xl),
        children: <Widget>[
          if (needsYou.isNotEmpty) ...<Widget>[
            PanelTitle(l10n.inboxNeedsYou),
            const SizedBox(height: Space.sm),
            for (final InboxItem item in needsYou)
              Padding(
                padding: const EdgeInsets.only(bottom: Space.md),
                child: InboxCard(
                  item: item,
                  busy: state.busy.contains(item.id),
                  error: state.errors[item.id],
                  declined: state.declined.contains(item.id),
                  lead: item.id == needsYou.first.id,
                ),
              ),
          ],
          if (done.isNotEmpty) ...<Widget>[
            const SizedBox(height: Space.md),
            PanelTitle(l10n.inboxDone),
            const SizedBox(height: Space.sm),
            for (final InboxItem item in done)
              Padding(
                padding: const EdgeInsets.only(bottom: Space.md),
                child: InboxCard(item: item, busy: false),
              ),
          ],
        ],
      ),
    );
  }
}

class InboxCard extends ConsumerStatefulWidget {
  const InboxCard({super.key, required this.item, required this.busy, this.error, this.declined = false, this.lead = false});

  final InboxItem item;
  final bool busy;
  final BridgeException? error;
  final bool declined;
  final bool lead;

  @override
  ConsumerState<InboxCard> createState() => _InboxCardState();
}

class _InboxCardState extends ConsumerState<InboxCard> {
  bool _asking = false;

  InboxItem get item => widget.item;
  bool get busy => widget.busy;
  BridgeException? get error => widget.error;
  bool get declined => widget.declined;
  bool get lead => widget.lead;

  String _result(AppL10n l10n) {
    switch (item.state) {
      case 'answered':
        final String action = item.answerLabel() ?? item.answer ?? '';
        return item.answeredBy == 'phone' ? l10n.inboxAnsweredHere(action) : l10n.inboxAnsweredOnMac(action);
      case 'expired':
        return l10n.itemExpired;
      default:
        return l10n.itemWithdrawn;
    }
  }

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final Kit kit = Kit.of(context);
    return Semantics(
      container: true,
      label: item.title,
      child: Surface(
        level: 2,
        radius: BorderRadius.circular(Radii.md),
        outline: item.isOpen ? kit.needs.withValues(alpha: 0.5) : null,
        padding: const EdgeInsets.all(Space.md),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: <Widget>[
            Text(item.title, style: kit.heading),
            if (item.detail.isNotEmpty) ...<Widget>[
              const SizedBox(height: Space.xs),
              Text(item.detail, style: kit.bodySm),
            ],
            const SizedBox(height: Space.md),
            if (item.isOpen)
              Wrap(
                spacing: Space.sm,
                runSpacing: Space.sm,
                children: <Widget>[
                  for (final InboxAction action in item.actions) _button(context, l10n, action),
                  if (noPrimaryKinds.contains(item.kind))
                    SecondaryButton(
                      label: l10n.machinesChoose,
                      icon: Icons.computer,
                      onPressed: () => unawaited(showMachinesSheet(context, ref)),
                    ),
                ],
              )
            else
              Text(_result(l10n), style: kit.bodySm),
            if (declined) ...<Widget>[
              const SizedBox(height: Space.sm),
              Text(l10n.inboxDeclined, style: kit.bodySm),
            ],
            if (error != null) ...<Widget>[
              const SizedBox(height: Space.sm),
              Text(bridgeErrorLine(l10n, error!), style: kit.bodySm.copyWith(color: kit.danger)),
            ],
          ],
        ),
      ),
    );
  }

  Future<void> _answer(BuildContext context, InboxAction action) async {
    if (_asking) return;
    final BridgeClient? client = ref.read(bridgeClientProvider);
    setState(() => _asking = true);
    try {
      final bool yes = await confirmInboxAction(context, ref, item, action);
      if (!yes || !context.mounted) return;
      await ref.read(inboxProvider.notifier).answer(item, action.id, expected: client);
    } finally {
      if (mounted) setState(() => _asking = false);
    }
  }

  Widget _button(BuildContext context, AppL10n l10n, InboxAction action) {
    if (action.id == 'open') {
      return SecondaryButton(label: l10n.inboxOpenOnMac, icon: Icons.laptop_mac, onPressed: null);
    }
    final VoidCallback? press = busy || _asking ? null : () => unawaited(_answer(context, action));
    return action.primary && lead
        ? PrimaryButton(label: action.label, semanticLabel: '${action.label}, ${item.title}', onPressed: press)
        : SecondaryButton(label: action.label, semanticLabel: '${action.label}, ${item.title}', onPressed: press);
  }
}
