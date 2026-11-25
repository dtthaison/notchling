import 'package:flutter/widgets.dart';

import '../l10n/app_localizations.dart';
import '../l10n/app_localizations_en.dart';

abstract final class L10nHolder {
  static AppL10n _current = AppL10nEn();
  static bool _watching = false;

  static AppL10n get current => _current;

  static void update(List<Locale>? locales) {
    final Locale resolved = basicLocaleListResolution(locales, AppL10n.supportedLocales);
    _current = lookupAppL10n(resolved);
  }

  static void watch() {
    if (_watching) return;
    _watching = true;
    update(WidgetsBinding.instance.platformDispatcher.locales);
    WidgetsBinding.instance.addObserver(_LocaleObserver());
  }
}

class _LocaleObserver with WidgetsBindingObserver {
  @override
  void didChangeLocales(List<Locale>? locales) => L10nHolder.update(locales);
}
