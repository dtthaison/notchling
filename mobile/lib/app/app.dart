import 'package:flutter/material.dart';
import 'package:flutter_localizations/flutter_localizations.dart';

import '../l10n/app_localizations.dart';
import 'shell.dart';
import 'theme/tokens.dart';

class AgentConsoleApp extends StatelessWidget {
  const AgentConsoleApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Agent Console',
      debugShowCheckedModeBanner: false,
      theme: buildTheme(),
      themeMode: ThemeMode.dark,
      darkTheme: buildTheme(),
      localizationsDelegates: const <LocalizationsDelegate<Object?>>[
        AppL10n.delegate,
        GlobalMaterialLocalizations.delegate,
        GlobalWidgetsLocalizations.delegate,
        GlobalCupertinoLocalizations.delegate,
      ],
      supportedLocales: AppL10n.supportedLocales,
      // Vietnamese is the default; English only when the device asks for it.
      localeResolutionCallback: (Locale? locale, Iterable<Locale> supported) =>
          locale?.languageCode == 'en' ? const Locale('en') : const Locale('vi'),
      home: const AppRoot(),
    );
  }
}
