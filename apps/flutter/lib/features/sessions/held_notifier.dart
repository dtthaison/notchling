import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/net/bridge_client.dart';
import '../../core/protocol/generated.dart' as wire;
import '../connection/connection.dart';

class HeldState {
  const HeldState({this.entries = const <wire.HeldEntry>[], this.notices = const <wire.HeldNotice>[]});
  final List<wire.HeldEntry> entries;
  final List<wire.HeldNotice> notices;
}

final Provider<List<bool>> heldPendingProvider = Provider<List<bool>>((Ref ref) => <bool>[false]);

final NotifierProvider<HeldNotifier, HeldState> heldProvider = NotifierProvider<HeldNotifier, HeldState>(HeldNotifier.new);

class HeldNotifier extends Notifier<HeldState> {
  final Set<String> _dismissed = <String>{};

  @override
  HeldState build() {
    final BridgeClient? client = ref.watch(bridgeClientProvider);
    ref.watch(connectedCountProvider);
    if (client == null) return const HeldState();
    final StreamSubscription<BridgeEvent> events = client.events.listen((BridgeEvent event) {
      switch (event) {
        case HeldEvent(:final List<wire.HeldEntry> entries, :final List<wire.HeldNotice> notices):
          _set(entries, notices);
        case HeldHintEvent():
          unawaited(_load(client));
        default:
          break;
      }
    });
    ref.onDispose(events.cancel);
    if (client.isConnected && client.inCluster) unawaited(_load(client));
    return const HeldState();
  }

  void _set(List<wire.HeldEntry> entries, List<wire.HeldNotice> notices) {
    ref.read(heldPendingProvider)[0] = entries.isNotEmpty;
    state = HeldState(entries: entries, notices: <wire.HeldNotice>[for (final wire.HeldNotice n in notices) if (!_dismissed.contains(n.id)) n]);
  }

  Future<void> _load(BridgeClient client) async {
    try {
      final ({List<wire.HeldEntry> entries, List<wire.HeldNotice> notices}) held = await client.heldList();
      if (!ref.mounted) return;
      _set(held.entries, held.notices);
    } on BridgeException catch (_) {
    }
  }

  Future<void> cancel(String heldId) => _act((BridgeClient client) => client.heldCancel(heldId));

  Future<void> confirm(String heldId) => _act((BridgeClient client) => client.heldConfirm(heldId));

  void dismiss(String noticeId) {
    _dismissed.add(noticeId);
    state = HeldState(entries: state.entries, notices: <wire.HeldNotice>[for (final wire.HeldNotice n in state.notices) if (n.id != noticeId) n]);
  }

  Future<void> _act(Future<bool> Function(BridgeClient client) run) async {
    final BridgeClient? client = ref.read(bridgeClientProvider);
    if (client == null) return;
    try {
      await run(client);
    } on BridgeException catch (_) {
    }
    if (ref.mounted) await _load(client);
  }
}
