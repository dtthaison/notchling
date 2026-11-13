import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_localizations/flutter_localizations.dart';

import '../features/settings/app_settings.dart';
import '../l10n/app_localizations.dart';
import 'nav_state.dart';
import 'shell.dart';
import 'widgets/alert_banner.dart';
import 'theme/tokens.dart';

class AgentConsoleApp extends ConsumerWidget {
  const AgentConsoleApp({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final ThemeMode mode = ref.watch(
      appSettingsProvider.select((AppSettings s) => s.themeMode),
    );
    return MaterialApp(
      title: 'Agent Console',
      debugShowCheckedModeBanner: false,
      theme: buildTheme(Brightness.light),
      themeMode: mode,
      // Changing the theme crossfades every colour (Kit.lerp) instead of flipping.
      themeAnimationDuration: const Duration(milliseconds: 380),
      themeAnimationCurve: Curves.easeOutCubic,
      darkTheme: buildTheme(Brightness.dark),
      localizationsDelegates: const <LocalizationsDelegate<Object?>>[
        AppL10n.delegate,
        GlobalMaterialLocalizations.delegate,
        GlobalWidgetsLocalizations.delegate,
        GlobalCupertinoLocalizations.delegate,
      ],
      supportedLocales: AppL10n.supportedLocales,
      // Vietnamese is the default; English only when the device asks for it.
      localeResolutionCallback: (Locale? locale, Iterable<Locale> supported) =>
          locale?.languageCode == 'en'
          ? const Locale('en')
          : const Locale('vi'),
      navigatorKey: appNavigatorKey,
      // Above the Navigator, so neither the app bar nor a pushed chat (with its own header) draws over the alert.
      builder: (BuildContext context, Widget? child) => Stack(
        children: <Widget>[
          ?child,
          Positioned(
            top: MediaQuery.paddingOf(context).top,
            left: 0,
            right: 0,
            child: const Material(type: MaterialType.transparency, child: InAppAlertBanner()),
          ),
        ],
      ),
      home: const AppRoot(),
    );
  }
}
