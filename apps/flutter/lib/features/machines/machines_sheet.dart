import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/tokens.dart';
import '../../core/net/bridge_client.dart';
import '../../l10n/app_localizations.dart';
import '../connection/bridge_errors.dart';
import '../connection/connection.dart';
import 'machines_models.dart';
import 'machines_notifier.dart';

Future<void> showMachinesSheet(BuildContext context, WidgetRef ref) async {
  List<PromoteMachine>? machines;
  try {
    machines = await ref.read(machinesProvider.future);
  } on BridgeException {
    return;
  }
  if (machines == null || !context.mounted) return;
  await showModalBottomSheet<void>(
    context: context,
    isScrollControlled: true,
    useSafeArea: true,
    showDragHandle: true,
    builder: (BuildContext context) => MachinesSheet(machines: machines!),
  );
}

class MachinesSheet extends ConsumerStatefulWidget {
  const MachinesSheet({super.key, required this.machines});

  final List<PromoteMachine> machines;

  @override
  ConsumerState<MachinesSheet> createState() => _MachinesSheetState();
}

class _MachinesSheetState extends ConsumerState<MachinesSheet> {
  bool _sending = false;
  String? _message;
  bool _failed = false;

  Future<bool> _ask({required String title, required String body, required String action}) async {
    final AppL10n l10n = AppL10n.of(context);
    final bool? yes = await showDialog<bool>(
      context: context,
      builder: (BuildContext context) => AlertDialog(
        title: Text(title),
        content: Text(body),
        actions: <Widget>[
          TextButton(onPressed: () => Navigator.pop(context, false), child: Text(l10n.appCancel)),
          TextButton(onPressed: () => Navigator.pop(context, true), child: Text(action)),
        ],
      ),
    );
    return yes == true;
  }

  String _confirmBody(AppL10n l10n, PromoteMachine machine) {
    final DateTime? at = machine.setAt;
    if (!machine.canResume || at == null) return l10n.machinesConfirmBodyJobs(machine.name);
    final String? old = ref.read(connectionStatusProvider).primaryName;
    return old == null ? l10n.machinesConfirmBodyUnnamed(clockText(at)) : l10n.machinesConfirmBody(clockText(at), old);
  }

  Future<void> _pick(PromoteMachine machine) async {
    if (_sending || !machine.pickable) return;
    final AppL10n l10n = AppL10n.of(context);
    final BridgeClient? client = ref.read(bridgeClientProvider);
    if (client == null) return;
    final bool yes = await _ask(
      title: l10n.machinesConfirmTitle(machine.name),
      body: _confirmBody(l10n, machine),
      action: l10n.machinesMove,
    );
    if (!yes || !mounted) return;
    await _send(client, machine, confirmed: false);
  }

  Future<void> _send(BridgeClient client, PromoteMachine machine, {required bool confirmed}) async {
    final AppL10n l10n = AppL10n.of(context);
    setState(() {
      _sending = true;
      _message = null;
    });
    try {
      final PromoteAnswer answer = await promoteMachine(client, machine.id, confirmed: confirmed);
      if (!mounted) return;
      final String? risk = answer.needsConfirm;
      if (risk != null) {
        setState(() => _sending = false);
        final bool yes = await _ask(title: l10n.machinesRiskTitle, body: risk, action: l10n.machinesMoveAnyway);
        if (yes && mounted) await _send(client, machine, confirmed: true);
        return;
      }
      setState(() {
        _sending = false;
        _failed = false;
        _message = answer.relayed
            ? ((answer.text ?? '').isEmpty ? l10n.machinesRelayedFallback : answer.text)
            : l10n.machinesMoving(machine.name);
      });
    } on BridgeException catch (error) {
      if (!mounted) return;
      setState(() {
        _sending = false;
        _failed = true;
        _message = '${l10n.machinesFailed}\n${bridgeErrorLine(l10n, error)}';
      });
    }
  }

  String _subtitle(AppL10n l10n, PromoteMachine machine) {
    if (!machine.canBeMaster) return l10n.machinesNotMaster;
    if (!machine.ready) return l10n.machinesNotReady;
    final DateTime? at = machine.setAt;
    if (machine.canResume && at != null) return l10n.machinesResume(clockText(at));
    return l10n.machinesJobsOnly;
  }

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final Kit kit = Kit.of(context);
    final List<PromoteMachine> online = widget.machines.where((PromoteMachine m) => m.online).toList();
    final String? message = _message;
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        Padding(padding: const EdgeInsets.symmetric(horizontal: Space.lg), child: Text(l10n.machinesTitle, style: kit.titleMd)),
        Padding(
          padding: const EdgeInsets.fromLTRB(Space.lg, Space.xs, Space.lg, Space.md),
          child: Text(l10n.machinesIntro, style: kit.bodySm),
        ),
        Flexible(
          child: online.isEmpty
              ? Padding(padding: const EdgeInsets.all(Space.lg), child: Text(l10n.machinesEmpty, style: kit.bodyMd))
              : ListView(
                  shrinkWrap: true,
                  children: <Widget>[for (final PromoteMachine machine in online) _row(context, l10n, kit, machine)],
                ),
        ),
        if (message != null)
          Padding(
            padding: const EdgeInsets.all(Space.lg),
            child: Semantics(
              liveRegion: true,
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: <Widget>[
                  Icon(_failed ? Icons.error_outline : Icons.check_circle_outline, size: 18, color: _failed ? kit.danger : kit.working),
                  const SizedBox(width: Space.sm),
                  Expanded(child: Text(message, style: kit.bodySm.copyWith(color: _failed ? kit.danger : kit.text))),
                ],
              ),
            ),
          ),
        const SizedBox(height: Space.lg),
      ],
    );
  }

  Widget _row(BuildContext context, AppL10n l10n, Kit kit, PromoteMachine machine) {
    final bool enabled = machine.pickable && !_sending;
    final String subtitle = _subtitle(l10n, machine);
    return Semantics(
      button: true,
      enabled: machine.pickable,
      label: '${machine.name}, $subtitle',
      excludeSemantics: true,
      onTap: enabled ? () => unawaited(_pick(machine)) : null,
      child: InkWell(
        onTap: enabled ? () => unawaited(_pick(machine)) : null,
        child: ConstrainedBox(
          constraints: const BoxConstraints(minHeight: Metric.minTarget),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: Space.lg, vertical: Space.sm),
            child: Row(
              children: <Widget>[
                Icon(Icons.computer, size: 20, color: machine.pickable ? kit.accent : kit.textMuted),
                const SizedBox(width: Space.md),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: <Widget>[
                      Text(machine.name, style: kit.heading.copyWith(color: machine.pickable ? kit.text : kit.textMuted)),
                      Text(subtitle, style: kit.bodySm),
                    ],
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
