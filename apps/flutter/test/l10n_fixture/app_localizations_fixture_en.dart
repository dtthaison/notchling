// ignore: unused_import
import 'package:intl/intl.dart' as intl;
import 'app_localizations_fixture.dart';

// ignore_for_file: type=lint

/// The translations for English (`en`).
class FixtureL10nEn extends FixtureL10n {
  FixtureL10nEn([String locale = 'en']) : super(locale);

  @override
  String get apostrophe => 'Couldn\'t load \"it\" · it\'s “fine”';

  @override
  String plural(int count) {
    final intl.NumberFormat countNumberFormat =
        intl.NumberFormat.decimalPattern(localeName);
    final String countString = countNumberFormat.format(count);

    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$countString sessions aren\'t running',
      one: '$countString session isn\'t running',
    );
    return '$_temp0';
  }

  @override
  String hash(int count) {
    final intl.NumberFormat countNumberFormat =
        intl.NumberFormat.decimalPattern(localeName);
    final String countString = countNumberFormat.format(count);

    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '# sessions ($countString)',
      one: '# session ($countString)',
    );
    return '$_temp0';
  }

  @override
  String number(int count) {
    final intl.NumberFormat countNumberFormat =
        intl.NumberFormat.decimalPattern(localeName);
    final String countString = countNumberFormat.format(count);

    return '$countString open';
  }
}
