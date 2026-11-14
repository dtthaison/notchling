import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:intl/intl.dart' as intl;

import 'app_localizations_fixture_en.dart';

// ignore_for_file: type=lint

/// Callers can lookup localized strings with an instance of FixtureL10n
/// returned by `FixtureL10n.of(context)`.
///
/// Applications need to include `FixtureL10n.delegate()` in their app's
/// `localizationDelegates` list, and the locales they support in the app's
/// `supportedLocales` list. For example:
///
/// ```dart
/// import 'l10n/app_localizations_fixture.dart';
///
/// return MaterialApp(
///   localizationsDelegates: FixtureL10n.localizationsDelegates,
///   supportedLocales: FixtureL10n.supportedLocales,
///   home: MyApplicationHome(),
/// );
/// ```
///
/// ## Update pubspec.yaml
///
/// Please make sure to update your pubspec.yaml to include the following
/// packages:
///
/// ```yaml
/// dependencies:
///   # Internationalization support.
///   flutter_localizations:
///     sdk: flutter
///   intl: any # Use the pinned version from flutter_localizations
///
///   # Rest of dependencies
/// ```
///
/// ## iOS Applications
///
/// iOS applications define key application metadata, including supported
/// locales, in an Info.plist file that is built into the application bundle.
/// To configure the locales supported by your app, you’ll need to edit this
/// file.
///
/// First, open your project’s ios/Runner.xcworkspace Xcode workspace file.
/// Then, in the Project Navigator, open the Info.plist file under the Runner
/// project’s Runner folder.
///
/// Next, select the Information Property List item, select Add Item from the
/// Editor menu, then select Localizations from the pop-up menu.
///
/// Select and expand the newly-created Localizations item then, for each
/// locale your application supports, add a new item and select the locale
/// you wish to add from the pop-up menu in the Value field. This list should
/// be consistent with the languages listed in the FixtureL10n.supportedLocales
/// property.
abstract class FixtureL10n {
  FixtureL10n(String locale)
    : localeName = intl.Intl.canonicalizedLocale(locale.toString());

  final String localeName;

  static FixtureL10n of(BuildContext context) {
    return Localizations.of<FixtureL10n>(context, FixtureL10n)!;
  }

  static const LocalizationsDelegate<FixtureL10n> delegate =
      _FixtureL10nDelegate();

  /// A list of this localizations delegate along with the default localizations
  /// delegates.
  ///
  /// Returns a list of localizations delegates containing this delegate along with
  /// GlobalMaterialLocalizations.delegate, GlobalCupertinoLocalizations.delegate,
  /// and GlobalWidgetsLocalizations.delegate.
  ///
  /// Additional delegates can be added by appending to this list in
  /// MaterialApp. This list does not have to be used at all if a custom list
  /// of delegates is preferred or required.
  static const List<LocalizationsDelegate<dynamic>> localizationsDelegates =
      <LocalizationsDelegate<dynamic>>[
        delegate,
        GlobalMaterialLocalizations.delegate,
        GlobalCupertinoLocalizations.delegate,
        GlobalWidgetsLocalizations.delegate,
      ];

  /// A list of this localizations delegate's supported locales.
  static const List<Locale> supportedLocales = <Locale>[Locale('en')];

  /// No description provided for @apostrophe.
  ///
  /// In en, this message translates to:
  /// **'Couldn\'t load \"it\" · it\'s “fine”'**
  String get apostrophe;

  /// No description provided for @plural.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, one{{count} session isn\'t running} other{{count} sessions aren\'t running}}'**
  String plural(int count);

  /// No description provided for @hash.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, one{# session ({count})} other{# sessions ({count})}}'**
  String hash(int count);

  /// No description provided for @number.
  ///
  /// In en, this message translates to:
  /// **'{count} open'**
  String number(int count);
}

class _FixtureL10nDelegate extends LocalizationsDelegate<FixtureL10n> {
  const _FixtureL10nDelegate();

  @override
  Future<FixtureL10n> load(Locale locale) {
    return SynchronousFuture<FixtureL10n>(lookupFixtureL10n(locale));
  }

  @override
  bool isSupported(Locale locale) =>
      <String>['en'].contains(locale.languageCode);

  @override
  bool shouldReload(_FixtureL10nDelegate old) => false;
}

FixtureL10n lookupFixtureL10n(Locale locale) {
  // Lookup logic when only language code is specified.
  switch (locale.languageCode) {
    case 'en':
      return FixtureL10nEn();
  }

  throw FlutterError(
    'FixtureL10n.delegate failed to load unsupported locale "$locale". This is likely '
    'an issue with the localizations generation tool. Please file an issue '
    'on GitHub with a reproducible sample app and the gen-l10n configuration '
    'that was used.',
  );
}
