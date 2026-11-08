import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/providers.dart';
import '../../core/net/bridge_client.dart';
import '../../core/net/ws_transport.dart';
import '../../core/pairing/mac_profile.dart';
import '../macs/macs_notifier.dart';

/// The client of one paired computer (by `MacProfile.id`), with its own token and connection, running while the
/// computer stays paired. Null when it isn't. Every op on a session goes through the client of the session's
/// own computer (decision 0012).
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

/// The client for the computer in use. Null when none is paired.
final Provider<BridgeClient?> bridgeClientProvider = Provider<BridgeClient?>((Ref ref) {
  final String? id = ref.watch(macsProvider.select((MacsState s) => s.active?.id));
  return id == null ? null : ref.watch(computerClientProvider(id));
});

/// One computer's link state, for its heading in the list.
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

/// Connecting / connected / disconnected for the banner.
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

/// Becomes true each time the link comes up, so data notifiers can reload.
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
