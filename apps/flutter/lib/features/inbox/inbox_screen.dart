import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/tokens.dart';
import '../../app/widgets/common.dart';
import '../../app/widgets/kit.dart';
import '../../core/protocol/models.dart';
import '../../l10n/app_localizations.dart';
import 'inbox_notifier.dart';

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

class InboxCard extends ConsumerWidget {
  const InboxCard({super.key, required this.item, required this.busy, this.error, this.declined = false, this.lead = false});

  final InboxItem item;
  final bool busy;
  final String? error;
  final bool declined;
  final bool lead;

  String _result(AppL10n l10n) {
    switch (item.state) {
      case 'answered':
        final String action = item.answerLabel() ?? item.answer ?? '';
        return item.answeredBy == 'phone' ? l10n.inboxAnsweredHere(action) : l10n.inboxAnsweredOnMac(action);
      case 'expired':
        return l10n.inboxExpired;
      default:
        return l10n.inboxWithdrawn;
    }
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
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
                children: _buttons(context, ref, l10n),
              )
            else
              Text(_result(l10n), style: kit.bodySm),
            if (declined) ...<Widget>[
              const SizedBox(height: Space.sm),
              Text(l10n.inboxDeclined, style: kit.bodySm),
            ],
            if (error != null) ...<Widget>[
              const SizedBox(height: Space.sm),
              Text(error!, style: kit.bodySm.copyWith(color: kit.danger)),
            ],
          ],
        ),
      ),
    );
  }

  static bool _allows(InboxAction action) => action.id.startsWith('allow');

  List<Widget> _buttons(BuildContext context, WidgetRef ref, AppL10n l10n) {
    final bool secret = item.kind == 'secret_approval';
    final List<Widget> out = <Widget>[];
    bool hinted = false;
    for (final InboxAction action in item.actions) {
      if (secret && _allows(action)) {
        if (!hinted) {
          out.add(SecondaryButton(label: l10n.inboxAllowOnMac, icon: Icons.laptop_mac, onPressed: null));
          hinted = true;
        }
        continue;
      }
      out.add(_button(context, ref, l10n, action));
    }
    return out;
  }

  Widget _button(BuildContext context, WidgetRef ref, AppL10n l10n, InboxAction action) {
    if (action.id == 'open') {
      return SecondaryButton(label: l10n.inboxOpenOnMac, icon: Icons.laptop_mac, onPressed: null);
    }
    final VoidCallback? press = busy ? null : () => unawaited(ref.read(inboxProvider.notifier).answer(item, action.id));
    return action.primary && lead
        ? PrimaryButton(label: action.label, semanticLabel: '${action.label}, ${item.title}', onPressed: press)
        : SecondaryButton(label: action.label, semanticLabel: '${action.label}, ${item.title}', onPressed: press);
  }
}
