import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/providers.dart';
import '../../core/net/bridge_client.dart';
import '../../core/net/ws_transport.dart';
import '../../core/pairing/mac_profile.dart';
import '../../core/pairing/pairing_payload.dart';
import '../macs/macs_notifier.dart';

enum PairingPhase { idle, waiting, failed }

enum PairingFailure { invalid, fingerprintMismatch, denied, unreachable }

class PairingUiState {
  const PairingUiState({this.phase = PairingPhase.idle, this.failure});
  final PairingPhase phase;
  final PairingFailure? failure;
}

final NotifierProvider<PairingController, PairingUiState> pairingControllerProvider =
    NotifierProvider<PairingController, PairingUiState>(PairingController.new);

class PairingController extends Notifier<PairingUiState> {
  @override
  PairingUiState build() => const PairingUiState();

  void reset() => state = const PairingUiState();

  void fail(PairingFailure failure) => state = PairingUiState(phase: PairingPhase.failed, failure: failure);

  Future<bool> pair(PairingPayload payload) async {
    state = const PairingUiState(phase: PairingPhase.waiting);
    try {
      final ({String token, String host, bool viaTunnel}) result = await pairWithMac(
        connector: ref.read(wsConnectorProvider),
        hosts: payload.hosts,
        port: payload.port,
        fingerprint: payload.fingerprint,
        code: payload.code,
        deviceName: ref.read(deviceNameProvider),
        platform: ref.read(platformNameProvider),
        tunnel: payload.tunnel,
      );
      await ref.read(macsProvider.notifier).add(MacProfile(
            id: payload.fingerprint.substring(0, 16),
            name: payload.name,
            hosts: payload.hosts,
            port: payload.port,
            fingerprint: payload.fingerprint,
            token: result.token,
            lastGoodHost: result.viaTunnel ? null : result.host,
            tunnel: payload.tunnel,
            lastGoodWasTunnel: result.viaTunnel,
          ));
      state = const PairingUiState();
      return true;
    } on FingerprintMismatch {
      fail(PairingFailure.fingerprintMismatch);
    } on BridgeException catch (error) {
      fail(error.isUnauthorized ? PairingFailure.denied : PairingFailure.unreachable);
    } on Object {
      fail(PairingFailure.unreachable);
    }
    return false;
  }
}
