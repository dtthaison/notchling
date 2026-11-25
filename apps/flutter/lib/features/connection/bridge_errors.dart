import '../../core/net/bridge_client.dart';
import '../../l10n/app_localizations.dart';

const Set<String> _explained = <String>{'disconnected', 'timeout', 'view_only', 'unauthorized'};

String bridgeErrorText(AppL10n l10n, BridgeException error) => switch (error.code) {
  'disconnected' => l10n.bridgeErrorDisconnected,
  'timeout' => l10n.bridgeErrorTimeout,
  'view_only' => l10n.bridgeErrorViewOnly,
  'unauthorized' => l10n.bridgeErrorUnauthorized,
  'not_found' => l10n.bridgeErrorNotFound,
  'invalid' => l10n.bridgeErrorInvalid,
  'busy' => l10n.bridgeErrorBusy,
  'not_reachable' => l10n.bridgeErrorNotReachable,
  'internal' => l10n.bridgeErrorInternal,
  _ => l10n.bridgeErrorUnknown,
};

String? bridgeErrorDetail(BridgeException error) {
  if (_explained.contains(error.code)) return null;
  final String detail = error.message.trim();
  return detail.isEmpty ? null : detail;
}

String bridgeErrorLine(AppL10n l10n, BridgeException error) {
  final String text = bridgeErrorText(l10n, error);
  final String? detail = bridgeErrorDetail(error);
  return detail == null ? text : l10n.bridgeErrorWithDetail(text, detail);
}
