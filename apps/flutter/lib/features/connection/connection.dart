import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/providers.dart';
import '../../core/net/bridge_client.dart';
import '../../core/net/ws_transport.dart';
import '../../core/pairing/mac_profile.dart';
import '../macs/macs_notifier.dart';

final computerClientProvider = Provider.family<BridgeClient?, String>((Ref ref, String hostId) {
  final String? key = ref.watch(macsProvider.select((MacsState s) => s.keyOf(hostId)));
  final MacProfile? mac = ref.read(macsProvider).byId(hostId);
  if (key == null || mac == null) return null;
  final BridgeClient client = BridgeClient(
    connector: ref.read(wsConnectorProvider),
    profile: mac,
    onGoodRoute: (Endpoint route) => unawaited(ref.read(macsProvider.notifier).rememberRoute(hostId, route)),
  )..start();
  final StreamSubscription<void> networkChanges = ref.read(networkChangesProvider).listen((void _) => client.networkChanged());
  ref.onDispose(networkChanges.cancel);
  ref.onDispose(() => unawaited(client.dispose()));
  return client;
});

final Provider<BridgeClient?> bridgeClientProvider = Provider<BridgeClient?>((Ref ref) {
  final String? id = ref.watch(macsProvider.select((MacsState s) => s.active?.id));
  return id == null ? null : ref.watch(computerClientProvider(id));
});

final computerStatusProvider = NotifierProvider.family<ComputerStatusNotifier, ConnectionStatus, String>(ComputerStatusNotifier.new);

class ComputerStatusNotifier extends Notifier<ConnectionStatus> {
  ComputerStatusNotifier(this.hostId);
  final String hostId;

  @override
  ConnectionStatus build() {
    final BridgeClient? client = ref.watch(computerClientProvider(hostId));
    if (client == null) return const ConnectionStatus(LinkPhase.disconnected);
    final StreamSubscription<ConnectionStatus> subscription = client.status.listen((ConnectionStatus next) => state = next);
    ref.onDispose(subscription.cancel);
    return client.currentStatus;
  }
}

final NotifierProvider<ConnectionStatusNotifier, ConnectionStatus> connectionStatusProvider =
    NotifierProvider<ConnectionStatusNotifier, ConnectionStatus>(ConnectionStatusNotifier.new);

class ConnectionStatusNotifier extends Notifier<ConnectionStatus> {
  @override
  ConnectionStatus build() {
    final BridgeClient? client = ref.watch(bridgeClientProvider);
    if (client == null) return const ConnectionStatus(LinkPhase.disconnected);
    final StreamSubscription<ConnectionStatus> subscription = client.status.listen((ConnectionStatus next) => state = next);
    ref.onDispose(subscription.cancel);
    return client.currentStatus;
  }
}

final NotifierProvider<ConnectedCountNotifier, int> connectedCountProvider =
    NotifierProvider<ConnectedCountNotifier, int>(ConnectedCountNotifier.new);

class ConnectedCountNotifier extends Notifier<int> {
  @override
  int build() {
    final BridgeClient? client = ref.watch(bridgeClientProvider);
    if (client == null) return 0;
    int count = client.isConnected ? 1 : 0;
    final StreamSubscription<ConnectionStatus> subscription = client.status.listen((ConnectionStatus s) {
      if (s.phase == LinkPhase.connected) state = ++count;
    });
    ref.onDispose(subscription.cancel);
    return count;
  }
}
