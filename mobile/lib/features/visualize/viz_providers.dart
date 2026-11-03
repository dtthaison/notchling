import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/net/bridge_client.dart';
import '../../core/protocol/json.dart';
import '../connection/connection.dart';
import '../sessions/sessions_notifier.dart';

enum VizTab {
  phone('Phone'),
  web('Web'),
  flow('Flow'),
  diagram('Diagram'),
  entity('Entity'),
  chart('Chart');

  const VizTab(this.title);
  final String title;

  static VizTab? byName(String? name) {
    for (final VizTab tab in VizTab.values) {
      if (tab.name == name) return tab;
    }
    return null;
  }
}

/// The tab the user chose for each session; sessions without a choice follow the Mac's suggestion.
final NotifierProvider<VizTabChoiceNotifier, Map<String, VizTab>> vizTabChoiceProvider =
    NotifierProvider<VizTabChoiceNotifier, Map<String, VizTab>>(VizTabChoiceNotifier.new);

class VizTabChoiceNotifier extends Notifier<Map<String, VizTab>> {
  @override
  Map<String, VizTab> build() => <String, VizTab>{};

  void choose(String sessionId, VizTab tab) => state = <String, VizTab>{...state, sessionId: tab};
}

/// viz.get for a session: the suggested tab, the web URL and the flow, diagram and chart specs.
/// It reloads when the session's activity moves; the previous value stays on screen meanwhile.
final vizProvider = FutureProvider.family<Json, String>((Ref ref, String sessionId) async {
  final BridgeClient? client = ref.watch(bridgeClientProvider);
  ref.watch(connectedCountProvider);
  ref.watch(sessionsProvider.select((SessionsState s) => s.byId(sessionId)?.lastActivity.millisecondsSinceEpoch));
  if (client == null) throw const BridgeException('disconnected', 'Chưa kết nối tới Mac.');
  return client.vizGet(sessionId);
});

/// The entity diagram costs a folder scan on the Mac, so it is asked for only when that tab opens.
final vizEntityProvider = FutureProvider.family<Json, String>((Ref ref, String sessionId) async {
  final BridgeClient? client = ref.watch(bridgeClientProvider);
  if (client == null) throw const BridgeException('disconnected', 'Chưa kết nối tới Mac.');
  final Json data = await client.vizGet(sessionId, entity: true);
  return data;
});
