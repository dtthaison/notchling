import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/net/bridge_client.dart';
import '../../core/protocol/json.dart';
import '../../core/protocol/models.dart';
import '../connection/connection.dart';
import '../inbox/inbox_notifier.dart';
import 'machines_models.dart';

const Set<String> noPrimaryKinds = <String>{'no_primary', 'primary_unreachable'};

const Duration promoteTimeout = Duration(seconds: 45);

final FutureProvider<List<PromoteMachine>?> machinesProvider = FutureProvider.autoDispose<List<PromoteMachine>?>((Ref ref) async {
  final BridgeClient? client = ref.watch(bridgeClientProvider);
  if (client == null) return null;
  try {
    final Json data = await client.request('cluster.machines');
    return data.objects('machines').map(PromoteMachine.fromJson).toList();
  } on BridgeException catch (error) {
    if (error.code == 'invalid' || error.code == 'not_found') return null;
    rethrow;
  }
});

final Provider<String?> machinesOfferProvider = Provider<String?>((Ref ref) {
  final List<InboxItem> items = ref.watch(inboxProvider.select((InboxState s) => s.items));
  for (final InboxItem item in items) {
    if (item.isOpen && noPrimaryKinds.contains(item.kind)) return item.id;
  }
  final ConnectionStatus status = ref.watch(connectionStatusProvider);
  if (status.phase == LinkPhase.connected && status.via != null && !status.primaryAnswers) return 'link';
  return null;
});

Future<PromoteAnswer> promoteMachine(BridgeClient client, String node, {bool confirmed = false}) async {
  final Json data = await client.request('cluster.promote', <String, Object?>{'node': node, if (confirmed) 'confirmed': true}, promoteTimeout);
  return PromoteAnswer.fromJson(data);
}
