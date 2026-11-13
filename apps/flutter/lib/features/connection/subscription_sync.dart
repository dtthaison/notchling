import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/net/bridge_client.dart';
import '../sessions/sessions_notifier.dart';
import 'connection.dart';

final NotifierProvider<PhoneWantedNotifier, bool> phoneWantedProvider = NotifierProvider<PhoneWantedNotifier, bool>(PhoneWantedNotifier.new);

class PhoneWantedNotifier extends Notifier<bool> {
  @override
  bool build() => false;

  void set(bool wanted) => state = wanted;
}

final NotifierProvider<PhoneErrorNotifier, String?> phoneErrorProvider = NotifierProvider<PhoneErrorNotifier, String?>(PhoneErrorNotifier.new);

class PhoneErrorNotifier extends Notifier<String?> {
  @override
  String? build() => null;

  void set(String? message) => state = message;
}

final Provider<void> subscriptionSyncProvider = Provider<void>((Ref ref) {
  final BridgeClient? client = ref.watch(bridgeClientProvider);
  final String? selected = ref.watch(selectedSessionIdProvider);
  final bool phone = ref.watch(phoneWantedProvider);
  ref.watch(connectedCountProvider);
  if (client == null) return;
  unawaited(_apply(ref, client, SubscriptionSpec(sessions: true, inbox: true, sessionId: selected, phoneFps: phone && selected != null ? 8 : null, phoneWidth: 360)));
});

final Provider<List<SubscriptionSpec?>> _lastSpecProvider = Provider<List<SubscriptionSpec?>>((Ref ref) => <SubscriptionSpec?>[null]);

Future<void> _apply(Ref ref, BridgeClient client, SubscriptionSpec spec) async {
  final List<SubscriptionSpec?> last = ref.read(_lastSpecProvider);
  try {
    if (last[0]?.wantsPhone == true && !spec.wantsPhone && client.isConnected) await client.unsubscribe();
    last[0] = spec;
    await client.subscribe(spec);
    ref.read(phoneErrorProvider.notifier).set(null);
  } on BridgeException catch (error) {
    if (spec.wantsPhone) ref.read(phoneErrorProvider.notifier).set(error.message);
  }
}
