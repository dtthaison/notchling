import 'package:agent_console/core/net/bridge_client.dart';
import 'package:agent_console/features/connection/bridge_errors.dart';
import 'package:agent_console/l10n/app_localizations_en.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  final AppL10nEn l10n = AppL10nEn();

  test('every_client_code_has_words', () {
    for (final String code in <String>['disconnected', 'timeout', 'internal', 'view_only', 'unauthorized', 'not_found', 'invalid', 'busy', 'not_reachable']) {
      final String text = bridgeErrorText(l10n, BridgeException(code, ''));
      expect(text, isNotEmpty, reason: code);
      expect(text, isNot(code), reason: code);
      expect(text, isNot(contains('_')), reason: code);
      expect(bridgeErrorDetail(BridgeException(code, '')), isNull, reason: code);
    }
  });

  test('known_daemon_code_ignores_message', () {
    final String plain = bridgeErrorText(l10n, const BridgeException('busy', ''));
    expect(bridgeErrorText(l10n, const BridgeException('busy', 'Mac bận')), plain);
    expect(bridgeErrorText(l10n, const BridgeException('busy', 'The Mac is busy.')), plain);
    expect(bridgeErrorDetail(const BridgeException('view_only', 'Chỉ xem')), isNull);
    expect(bridgeErrorDetail(const BridgeException('unauthorized', 'Không được phép')), isNull);
  });

  test('unknown_code_shows_detail_under_general_message', () {
    const BridgeException error = BridgeException('quota_exceeded', 'The Mac ran out of room.');
    expect(bridgeErrorText(l10n, error), l10n.bridgeErrorUnknown);
    expect(bridgeErrorDetail(error), 'The Mac ran out of room.');
    expect(bridgeErrorDetail(const BridgeException('quota_exceeded', '')), isNull);
  });

  test('a_code_with_no_message_of_its_own_keeps_the_daemons_detail', () {
    const BridgeException error = BridgeException('invalid', 'The folder doesn\'t exist.');
    expect(bridgeErrorText(l10n, error), l10n.bridgeErrorInvalid);
    expect(bridgeErrorDetail(error), 'The folder doesn\'t exist.');
    expect(bridgeErrorDetail(const BridgeException('busy', 'Mac bận')), 'Mac bận');
  });

  test('line_joins_text_and_detail', () {
    expect(bridgeErrorLine(l10n, const BridgeException('timeout', '')), l10n.bridgeErrorTimeout);
    expect(
      bridgeErrorLine(l10n, const BridgeException('not_found', 'The folder doesn\'t exist.')),
      "The Mac couldn't find that. The folder doesn't exist.",
    );
  });
}
