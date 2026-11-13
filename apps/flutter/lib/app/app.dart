import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_localizations/flutter_localizations.dart';

import '../features/settings/app_settings.dart';
import '../l10n/app_localizations.dart';
import 'shell.dart';
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
      localeResolutionCallback: (Locale? locale, Iterable<Locale> supported) =>
          locale?.languageCode == 'en'
          ? const Locale('en')
          : const Locale('vi'),
      home: const AppRoot(),
    );
  }
}
