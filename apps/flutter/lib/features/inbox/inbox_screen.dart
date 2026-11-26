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
import 'inbox_alerts.dart';
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
    final List<InboxItem> blocking = state.blockingNow;
    final List<InboxItem> needsYou = state.needsYouRest;
    final List<InboxItem> done = state.done;
    final String? leadId = blocking.isNotEmpty ? blocking.first.id : (needsYou.isNotEmpty ? needsYou.first.id : null);
    Widget card(InboxItem item) => Padding(
      padding: const EdgeInsets.only(bottom: Space.md),
      child: InboxCard(
        item: item,
        busy: state.busy.contains(item.id),
        error: state.errors[item.id],
        declined: state.declined.contains(item.id),
        lead: item.id == leadId,
      ),
    );
    return RefreshIndicator(
      onRefresh: () => ref.read(inboxProvider.notifier).refresh(),
      child: ListView(
        padding: const EdgeInsets.fromLTRB(Space.lg, Space.lg, Space.lg, Space.xl),
        children: <Widget>[
          if (blocking.isNotEmpty) ...<Widget>[
            PanelTitle(l10n.inboxBlocking),
            const SizedBox(height: Space.sm),
            for (final InboxItem item in blocking) card(item),
          ],
          if (needsYou.isNotEmpty) ...<Widget>[
            if (blocking.isNotEmpty) const SizedBox(height: Space.md),
            PanelTitle(l10n.inboxNeedsYou),
            const SizedBox(height: Space.sm),
            for (final InboxItem item in needsYou) card(item),
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

  String? _outcome(AppL10n l10n) {
    final InboxOutcome? outcome = item.outcome;
    if (outcome == null) return null;
    final String reason = outcome.reason ?? '';
    switch (outcome.state) {
      case 'applying':
        return l10n.inboxOutcomeApplying;
      case 'applied':
        return l10n.inboxOutcomeApplied;
      case 'failed':
        return reason.isEmpty ? l10n.inboxOutcomeFailed : l10n.inboxOutcomeFailedReason(reason);
      case 'withdrawn':
        if (reason.isNotEmpty) return l10n.inboxOutcomeWithdrawnReason(reason);
        return item.state == 'withdrawn' ? null : l10n.inboxOutcomeWithdrawn;
      default:
        return null;
    }
  }

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
            if (item.isBlocking && item.waitingSince != null) ...<Widget>[
              const SizedBox(height: Space.xs),
              StoppedFor(item: item),
            ],
            if (item.place != null && item.place!.labels.isNotEmpty) ...<Widget>[
              const SizedBox(height: Space.sm),
              Wrap(
                spacing: Space.sm,
                runSpacing: Space.xs,
                children: <Widget>[for (final String label in item.place!.labels) PlaceChip(label: label)],
              ),
            ],
            const SizedBox(height: Space.md),
            if (item.isOpen)
              Wrap(
                spacing: Space.sm,
                runSpacing: Space.sm,
                children: <Widget>[
                  for (final InboxAction action in item.actions)
                    if (action.id != 'change') _button(context, l10n, action),
                  if (noPrimaryKinds.contains(item.kind))
                    SecondaryButton(
                      label: l10n.machinesChoose,
                      icon: Icons.computer,
                      onPressed: () => unawaited(showMachinesSheet(context, ref)),
                    ),
                ],
              )
            else ...<Widget>[
              Text(_result(l10n), style: kit.bodySm),
              if (_outcome(l10n) case final String line) ...<Widget>[
                const SizedBox(height: Space.xs),
                Text(line, style: kit.bodySm.copyWith(color: item.outcome?.state == 'failed' ? kit.danger : null)),
              ],
              if (item.answer == 'deny' && (item.note ?? '').isNotEmpty) ...<Widget>[
                const SizedBox(height: Space.xs),
                Text(l10n.inboxSentBack(item.note!), style: kit.bodySm),
              ],
            ],
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

class PlaceChip extends StatelessWidget {
  const PlaceChip({super.key, required this.label});

  final String label;

  @override
  Widget build(BuildContext context) {
    final Kit kit = Kit.of(context);
    return Container(
      constraints: const BoxConstraints(maxWidth: 220),
      padding: const EdgeInsets.symmetric(horizontal: Space.sm, vertical: Space.xs),
      decoration: BoxDecoration(color: kit.raised, borderRadius: BorderRadius.circular(999)),
      child: Text(label, style: kit.meta.copyWith(color: kit.textMuted), maxLines: 1, overflow: TextOverflow.ellipsis),
    );
  }
}

class StoppedFor extends StatefulWidget {
  const StoppedFor({super.key, required this.item, this.nowMs});

  final InboxItem item;
  final int Function()? nowMs;

  @override
  State<StoppedFor> createState() => _StoppedForState();
}

class _StoppedForState extends State<StoppedFor> {
  Timer? _timer;

  @override
  void initState() {
    super.initState();
    _timer = Timer.periodic(const Duration(seconds: 15), (Timer _) {
      if (mounted) setState(() {});
    });
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final int now = (widget.nowMs ?? () => DateTime.now().millisecondsSinceEpoch)();
    final int minutes = inboxMinutesWaiting(widget.item, now);
    final Kit kit = Kit.of(context);
    return Text(
      minutes < 1 ? l10n.inboxStoppedNow : l10n.inboxStoppedMinutes(minutes),
      style: kit.meta.copyWith(color: kit.needs),
    );
  }
}
