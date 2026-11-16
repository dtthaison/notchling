import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/tokens.dart';
import '../../core/net/bridge_client.dart';
import '../../l10n/app_localizations.dart';
import '../../app/notifications.dart' show lifecycleProvider;
import '../connection/bridge_errors.dart';
import '../connection/connection.dart';
import 'confirm_dialog.dart';
import 'machines_models.dart';
import 'machines_notifier.dart';

Future<bool> showMachinesSheet(BuildContext context, WidgetRef ref) async {
  final MachinesPopupNotifier popup = ref.read(machinesPopupProvider.notifier);
  if (!popup.begin()) return false;
  try {
    final BridgeClient? client = ref.read(bridgeClientProvider);
    if (client == null) return false;
    List<PromoteMachine>? machines;
    try {
      machines = await loadMachines(client);
    } on BridgeException {
      return false;
    }
    if (machines == null || !context.mounted || !identical(ref.read(bridgeClientProvider), client)) return false;
    await showModalBottomSheet<void>(
      context: context,
      isScrollControlled: true,
      useSafeArea: true,
      showDragHandle: true,
      builder: (BuildContext context) => MachinesSheet(machines: machines!, client: client),
    );
    return true;
  } finally {
    popup.end();
  }
}

void listenMachinesOffers(BuildContext context, WidgetRef ref) {
  ref.listen<MachinesOffer?>(machinesOfferProvider, (MachinesOffer? previous, MachinesOffer? next) {
    unawaited(offerMachinesSheet(context, ref));
  });
  ref.listen<AppLifecycleState>(lifecycleProvider, (AppLifecycleState? previous, AppLifecycleState next) {
    if (next == AppLifecycleState.resumed) unawaited(offerMachinesSheet(context, ref));
  });
}

Future<void> offerMachinesSheet(BuildContext context, WidgetRef ref) async {
  final MachinesPopupNotifier popup = ref.read(machinesPopupProvider.notifier);
  final MachinesOffer? offer = ref.read(machinesOfferProvider);
  if (offer == null) {
    if (ref.read(connectionStatusProvider).phase == LinkPhase.connected) popup.resolved();
    return;
  }
  if (ref.read(lifecycleProvider) != AppLifecycleState.resumed) return;
  final MachinesPopupState state = ref.read(machinesPopupProvider);
  if (state.busy || state.dismissed.contains(offer.hostId)) return;
  popup.dismiss(offer.hostId);
  await showMachinesSheet(context, ref);
}

class MachinesSheet extends ConsumerStatefulWidget {
  const MachinesSheet({super.key, required this.machines, required this.client});

  final List<PromoteMachine> machines;
  final BridgeClient client;

  @override
  ConsumerState<MachinesSheet> createState() => _MachinesSheetState();
}

class _MachinesSheetState extends ConsumerState<MachinesSheet> {
  bool _sending = false;
  bool _picking = false;
  List<String> _lines = const <String>[];
  bool _failed = false;

  bool _sameComputer() {
    if (identical(ref.read(bridgeClientProvider), widget.client)) return true;
    if (mounted) Navigator.of(context).maybePop();
    return false;
  }

  String _confirmBody(AppL10n l10n, PromoteMachine machine) {
    final DateTime? at = machine.setAt;
    if (!machine.canResume || at == null) return l10n.machinesConfirmBodyJobs(machine.name);
    final String? old = ref.read(connectionStatusProvider).primaryName;
    return old == null ? l10n.machinesConfirmBodyUnnamed(clockText(at)) : l10n.machinesConfirmBody(clockText(at), old);
  }

  Future<void> _pick(PromoteMachine machine) async {
    if (_sending || _picking || !machine.pickable) return;
    setState(() => _picking = true);
    try {
      final AppL10n l10n = AppL10n.of(context);
      final bool yes = await askConfirm(
        context,
        title: l10n.machinesConfirmTitle(machine.name),
        body: _confirmBody(l10n, machine),
        action: l10n.machinesMove,
      );
      if (!yes || !mounted || !_sameComputer()) return;
      await _send(machine, confirmed: false);
    } finally {
      if (mounted) setState(() => _picking = false);
    }
  }

  Future<void> _send(PromoteMachine machine, {required bool confirmed}) async {
    final AppL10n l10n = AppL10n.of(context);
    setState(() {
      _sending = true;
      _lines = const <String>[];
    });
    try {
      final PromoteAnswer answer = await promoteMachine(widget.client, machine.id, confirmed: confirmed);
      if (!mounted) return;
      final String? risk = answer.needsConfirm;
      if (risk != null) {
        setState(() => _sending = false);
        final bool yes = await askConfirm(context, title: l10n.machinesRiskTitle, body: risk, action: l10n.machinesMoveAnyway);
        if (yes && mounted && _sameComputer()) await _send(machine, confirmed: true);
        return;
      }
      setState(() {
        _sending = false;
        _failed = false;
        _lines = <String>[
          answer.relayed
              ? ((answer.text ?? '').isEmpty ? l10n.machinesRelayedFallback : answer.text!)
              : l10n.machinesMoving(machine.name),
        ];
      });
    } on BridgeException catch (error) {
      if (!mounted) return;
      setState(() {
        _sending = false;
        _failed = true;
        _lines = <String>[l10n.machinesFailed, bridgeErrorLine(l10n, error)];
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
    final String message = _lines.join('\n');
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
        if (_lines.isNotEmpty)
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
    final bool enabled = machine.pickable && !_sending && !_picking;
    final String subtitle = _subtitle(l10n, machine);
    return Semantics(
      button: true,
      enabled: machine.pickable,
      label: l10n.machinesRowLabel(machine.name, subtitle),
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
