import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/net/bridge_client.dart';
import '../sessions/sessions_notifier.dart';
import 'connection.dart';

/// True while the Phone tab is on screen: only then does the Mac stream device frames.
final NotifierProvider<PhoneWantedNotifier, bool> phoneWantedProvider = NotifierProvider<PhoneWantedNotifier, bool>(PhoneWantedNotifier.new);

class PhoneWantedNotifier extends Notifier<bool> {
  @override
  bool build() => false;

  void set(bool wanted) => state = wanted;
}

/// The error the Mac gave when it could not start the phone stream (no permission, no window).
final NotifierProvider<PhoneErrorNotifier, String?> phoneErrorProvider = NotifierProvider<PhoneErrorNotifier, String?>(PhoneErrorNotifier.new);

class PhoneErrorNotifier extends Notifier<String?> {
  @override
  String? build() => null;

  void set(String? message) => state = message;
}

/// Keeps what the Mac pushes in step with what the phone shows: always the session list, the chat of the
/// selected session, and device frames only while the Phone tab is open. Watch it once, near the root.
final Provider<void> subscriptionSyncProvider = Provider<void>((Ref ref) {
  final BridgeClient? client = ref.watch(bridgeClientProvider);
  final String? selected = ref.watch(selectedSessionIdProvider);
  final bool phone = ref.watch(phoneWantedProvider);
  ref.watch(connectedCountProvider);
  if (client == null) return;
  unawaited(_apply(ref, client, SubscriptionSpec(sessions: true, sessionId: selected, phoneFps: phone && selected != null ? 8 : null, phoneWidth: 360)));
});

/// The last spec sent, per container, so leaving the Phone tab can be told apart from the first subscribe.
final Provider<List<SubscriptionSpec?>> _lastSpecProvider = Provider<List<SubscriptionSpec?>>((Ref ref) => <SubscriptionSpec?>[null]);

Future<void> _apply(Ref ref, BridgeClient client, SubscriptionSpec spec) async {
  final List<SubscriptionSpec?> last = ref.read(_lastSpecProvider);
  try {
    // The Mac has no "stop the phone stream" op: leaving the Phone tab means unsubscribing and starting over.
    if (last[0]?.wantsPhone == true && !spec.wantsPhone && client.isConnected) await client.unsubscribe();
    last[0] = spec;
    await client.subscribe(spec);
    ref.read(phoneErrorProvider.notifier).set(null);
  } on BridgeException catch (error) {
    if (spec.wantsPhone) ref.read(phoneErrorProvider.notifier).set(error.message);
  }
}
