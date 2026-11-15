import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/net/bridge_client.dart';
import '../../core/protocol/generated.dart' as wire;
import '../connection/connection.dart';
import '../macs/macs_notifier.dart';

class HeldState {
  const HeldState({
    this.entries = const <wire.HeldEntry>[],
    this.notices = const <wire.HeldNotice>[],
    this.holder = '',
    this.failed = false,
  });
  final List<wire.HeldEntry> entries;
  final List<wire.HeldNotice> notices;
  final String holder;
  final bool failed;

  HeldState withFailed(bool value) => HeldState(entries: entries, notices: notices, holder: holder, failed: value);
}

final computerHeldProvider =
    NotifierProvider.family<ComputerHeldNotifier, HeldState, String>(ComputerHeldNotifier.new);

final Provider<HeldState> heldProvider = Provider<HeldState>((Ref ref) {
  final String? id = ref.watch(macsProvider.select((MacsState s) => s.active?.id));
  return id == null ? const HeldState() : ref.watch(computerHeldProvider(id));
});

final Provider<void> heldTrackingProvider = Provider<void>((Ref ref) {
  final String ids = ref.watch(macsProvider.select((MacsState s) => s.macs.map((m) => m.id).join('\n')));
  for (final String id in ids.split('\n')) {
    if (id.isNotEmpty) ref.watch(computerHeldProvider(id));
  }
});

class ComputerHeldNotifier extends Notifier<HeldState> {
  ComputerHeldNotifier(this.hostId);
  final String hostId;
  final Set<String> _dismissed = <String>{};

  @override
  HeldState build() {
    final BridgeClient? client = ref.watch(computerClientProvider(hostId));
    if (client == null) return const HeldState();
    final StreamSubscription<BridgeEvent> events = client.events.listen((BridgeEvent event) {
      switch (event) {
        case HeldEvent(:final List<wire.HeldEntry> entries, :final List<wire.HeldNotice> notices):
          _set(client, entries, notices);
        case HeldHintEvent():
          unawaited(_load(client));
        default:
          break;
      }
    });
    final StreamSubscription<ConnectionStatus> status = client.status.listen((ConnectionStatus next) {
      if (next.phase == LinkPhase.connected && client.inCluster) unawaited(_load(client));
    });
    ref.onDispose(events.cancel);
    ref.onDispose(status.cancel);
    if (client.isConnected && client.inCluster) unawaited(_load(client));
    return const HeldState();
  }

  void _set(BridgeClient client, List<wire.HeldEntry> entries, List<wire.HeldNotice> notices) {
    if (!ref.mounted) return;
    client.markHeld(entries.isNotEmpty);
    state = HeldState(
      entries: entries,
      notices: <wire.HeldNotice>[for (final wire.HeldNotice n in notices) if (!_dismissed.contains(n.id)) n],
      holder: client.nodeName,
      failed: state.failed,
    );
  }

  Future<void> _load(BridgeClient client) async {
    try {
      final ({List<wire.HeldEntry> entries, List<wire.HeldNotice> notices}) held = await client.heldList();
      _set(client, held.entries, held.notices);
    } on BridgeException catch (error) {
      if (!ref.mounted || error.code == 'disconnected' || error.code == 'timeout') return;
      client.markHeld(false);
      state = const HeldState();
    }
  }

  Future<void> reload() async {
    final BridgeClient? client = ref.read(computerClientProvider(hostId));
    if (client != null) await _load(client);
  }

  Future<void> cancel(String heldId) => _act((BridgeClient client) async {
        final ({bool done, bool delivered}) result = await client.heldCancel(heldId);
        return result.done || result.delivered;
      });

  Future<void> confirm(String heldId) => _act((BridgeClient client) => client.heldConfirm(heldId));

  void dismiss(String noticeId) {
    _dismissed.add(noticeId);
    state = HeldState(
      entries: state.entries,
      notices: <wire.HeldNotice>[for (final wire.HeldNotice n in state.notices) if (n.id != noticeId) n],
      holder: state.holder,
      failed: state.failed,
    );
  }

  Future<void> _act(Future<bool> Function(BridgeClient client) run) async {
    final BridgeClient? client = ref.read(computerClientProvider(hostId));
    if (client == null) {
      state = state.withFailed(true);
      return;
    }
    state = state.withFailed(false);
    bool ok;
    try {
      ok = await run(client);
    } on BridgeException catch (_) {
      ok = false;
    }
    if (!ref.mounted) return;
    await _load(client);
    if (ref.mounted && !ok) state = state.withFailed(true);
  }
}
