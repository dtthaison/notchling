import 'package:agent_console/l10n/app_localizations_en.dart';
import 'package:flutter_test/flutter_test.dart';

import 'l10n_fixture/app_localizations_fixture_en.dart';

void main() {
  test('apostrophe_survives_gen_l10n', () {
    expect(AppL10nEn().appLoadFailed, "Couldn't load data. Please try again.");
    expect(FixtureL10nEn().apostrophe, 'Couldn\'t load "it" · it\'s “fine”');
  });

  test('plural_one_and_other', () {
    final FixtureL10nEn l10n = FixtureL10nEn();
    expect(l10n.plural(1), "1 session isn't running");
    expect(l10n.plural(2), "2 sessions aren't running");
    expect(l10n.plural(1200), "1,200 sessions aren't running");
  });

  test('hash_in_plural_branch', () {
    final FixtureL10nEn l10n = FixtureL10nEn();
    expect(l10n.hash(1), '# session (1)');
    expect(l10n.hash(5), '# sessions (5)');
  });

  test('number_placeholder_keeps_its_value', () {
    expect(AppL10nEn().appInboxOpenCount(3), '3 open');
    expect(AppL10nEn().sessionsMinutesShort(12), '12 min');
  });
}
