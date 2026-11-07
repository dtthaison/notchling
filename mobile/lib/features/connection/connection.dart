import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/providers.dart';
import '../../core/net/bridge_client.dart';
import '../../core/net/ws_transport.dart';
import '../macs/macs_notifier.dart';

/// The client for the active Mac, running for as long as that Mac stays active. Null when none is paired.
final Provider<BridgeClient?> bridgeClientProvider = Provider<BridgeClient?>((Ref ref) {
  final String? key = ref.watch(macsProvider.select((MacsState s) => s.activeKey));
  if (key == null) return null;
  final String macId = ref.read(macsProvider).active!.id;
  final BridgeClient client = BridgeClient(
    connector: ref.read(wsConnectorProvider),
    profile: ref.read(macsProvider).active!,
    onGoodRoute: (Endpoint route) => unawaited(ref.read(macsProvider.notifier).rememberRoute(macId, route)),
  )..start();
  final StreamSubscription<void> networkChanges = ref.read(networkChangesProvider).listen((void _) => client.networkChanged());
  ref.onDispose(networkChanges.cancel);
  ref.onDispose(() => unawaited(client.dispose()));
  return client;
});

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
