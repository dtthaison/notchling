import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/net/bridge_client.dart';
import '../../core/protocol/json.dart';
import '../../core/protocol/models.dart';
import '../connection/connection.dart';
import '../inbox/inbox_notifier.dart';
import '../macs/macs_notifier.dart';
import 'machines_models.dart';

const Set<String> noPrimaryKinds = <String>{'no_primary', 'primary_unreachable'};

const Duration promoteTimeout = Duration(seconds: 45);

Future<List<PromoteMachine>?> loadMachines(BridgeClient client) async {
  try {
    final Json data = await client.request('cluster.machines');
    return data.objects('machines').map(PromoteMachine.fromJson).toList();
  } on BridgeException catch (error) {
    if (error.code == 'invalid' || error.code == 'not_found') return null;
    rethrow;
  }
}

class MachinesOffer {
  const MachinesOffer(this.hostId);

  final String hostId;

  @override
  bool operator ==(Object other) => other is MachinesOffer && other.hostId == hostId;

  @override
  int get hashCode => hostId.hashCode;
}

final Provider<MachinesOffer?> machinesOfferProvider = Provider<MachinesOffer?>((Ref ref) {
  final String? hostId = ref.watch(macsProvider.select((MacsState s) => s.active?.id));
  if (hostId == null) return null;
  final List<InboxItem> items = ref.watch(inboxProvider.select((InboxState s) => s.items));
  for (final InboxItem item in items) {
    if (item.isOpen && noPrimaryKinds.contains(item.kind)) return MachinesOffer(hostId);
  }
  final ConnectionStatus status = ref.watch(connectionStatusProvider);
  if (status.phase == LinkPhase.connected && status.via != null && status.primaryName != null && !status.primaryAnswers) {
    return MachinesOffer(hostId);
  }
  return null;
});

class MachinesPopupState {
  const MachinesPopupState({this.busy = false, this.dismissed = const <String>{}});

  final bool busy;
  final Set<String> dismissed;
}

final NotifierProvider<MachinesPopupNotifier, MachinesPopupState> machinesPopupProvider =
    NotifierProvider<MachinesPopupNotifier, MachinesPopupState>(MachinesPopupNotifier.new);

class MachinesPopupNotifier extends Notifier<MachinesPopupState> {
  @override
  MachinesPopupState build() => const MachinesPopupState();

  bool begin() {
    if (state.busy) return false;
    state = MachinesPopupState(busy: true, dismissed: state.dismissed);
    return true;
  }

  void end() => state = MachinesPopupState(dismissed: state.dismissed);

  void dismiss(String hostId) => state = MachinesPopupState(busy: state.busy, dismissed: <String>{...state.dismissed, hostId});

  void resolved() {
    if (state.dismissed.isEmpty) return;
    state = MachinesPopupState(busy: state.busy);
  }
}

Future<PromoteAnswer> promoteMachine(BridgeClient client, String node, {bool confirmed = false}) async {
  final Json data = await client.request('cluster.promote', <String, Object?>{'node': node, if (confirmed) 'confirmed': true}, promoteTimeout);
  return PromoteAnswer.fromJson(data);
}
