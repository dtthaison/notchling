import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/net/bridge_client.dart';
import '../../core/protocol/json.dart';
import '../../l10n/app_localizations.dart';
import '../connection/connection.dart';
import '../sessions/sessions_notifier.dart';

enum VizTab {
  phone,
  web,
  flow,
  diagram,
  entity,
  chart;

  String title(AppL10n l10n) => switch (this) {
    VizTab.phone => l10n.visualizeTabPhone,
    VizTab.web => l10n.visualizeTabWeb,
    VizTab.flow => l10n.visualizeTabFlow,
    VizTab.diagram => l10n.visualizeTabDiagram,
    VizTab.entity => l10n.visualizeTabEntity,
    VizTab.chart => l10n.visualizeTabChart,
  };

  static List<VizTab> get shown => <VizTab>[for (final VizTab tab in values) if (tab != phone) tab];

  static VizTab? byName(String? name) {
    for (final VizTab tab in VizTab.values) {
      if (tab.name == name) return tab;
    }
    return null;
  }
}

final NotifierProvider<VizTabChoiceNotifier, Map<String, VizTab>> vizTabChoiceProvider =
    NotifierProvider<VizTabChoiceNotifier, Map<String, VizTab>>(VizTabChoiceNotifier.new);

class VizTabChoiceNotifier extends Notifier<Map<String, VizTab>> {
  @override
  Map<String, VizTab> build() => <String, VizTab>{};

  void choose(String sessionId, VizTab tab) => state = <String, VizTab>{...state, sessionId: tab};
}

final vizProvider = FutureProvider.family<Json, String>((Ref ref, String sessionId) async {
  final BridgeClient? client = ref.watch(bridgeClientProvider);
  ref.watch(connectedCountProvider);
  ref.watch(sessionsProvider.select((SessionsState s) => s.byId(sessionId)?.lastActivity.millisecondsSinceEpoch));
  if (client == null) throw const BridgeException('disconnected', '');
  return client.vizGet(sessionId);
});

final vizEntityProvider = FutureProvider.family<Json, String>((Ref ref, String sessionId) async {
  final BridgeClient? client = ref.watch(bridgeClientProvider);
  if (client == null) throw const BridgeException('disconnected', '');
  final Json data = await client.vizGet(sessionId, entity: true);
  return data;
});
