// Golden images of the chat kit: a user bubble, a Claude card and the composer, light and dark.
// Flutter's test font draws every glyph as a box, so the images match on any machine.
// Changing a golden PNG is a protected change: it needs the maintainer.
// Update: flutter test --update-goldens test/goldens

import 'package:agent_console/app/theme/tokens.dart';
import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/chat/chat_notifier.dart';
import 'package:agent_console/features/chat/chat_parts.dart';
import 'package:agent_console/features/chat/composer.dart';
import 'package:agent_console/features/chat/slash_commands.dart';
import 'package:agent_console/l10n/app_localizations.dart';
import 'package:flutter/material.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

final DateTime _time = DateTime.utc(2026, 11, 7, 9, 30);

Widget _kit(Brightness brightness, TextEditingController controller) {
  return ProviderScope(
    child: MaterialApp(
      debugShowCheckedModeBanner: false,
      theme: buildTheme(brightness),
      locale: const Locale('en'),
      localizationsDelegates: const <LocalizationsDelegate<Object?>>[
        AppL10n.delegate,
        GlobalMaterialLocalizations.delegate,
        GlobalWidgetsLocalizations.delegate,
        GlobalCupertinoLocalizations.delegate,
      ],
      supportedLocales: AppL10n.supportedLocales,
      home: Scaffold(
        body: Padding(
          padding: const EdgeInsets.all(16),
          child: Column(
            children: <Widget>[
              UserBubble(
                sessionId: 's1',
                message: ChatMessage(id: 'u1', role: ChatRole.user, text: 'Fix the login test', time: _time),
              ),
              const SizedBox(height: 12),
              AssistantCard(
                sessionId: 's1',
                message: ChatMessage(id: 'a1', role: ChatRole.assistant, text: 'Done. The test passes now.', time: _time),
              ),
              const Spacer(),
              Composer(
                controller: controller,
                send: const SendIdle(),
                viewOnly: false,
                onSend: () {},
                onRetry: () {},
                onDismissError: () {},
                commands: builtInSlashCommands,
                folder: '~/code/api',
              ),
            ],
          ),
        ),
      ),
    ),
  );
}

void main() {
  for (final (Brightness brightness, String name) in <(Brightness, String)>[
    (Brightness.light, 'light'),
    (Brightness.dark, 'dark'),
  ]) {
    testWidgets('chat kit bubble and composer, $name', (WidgetTester tester) async {
      tester.view.physicalSize = const Size(390, 520);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      final TextEditingController controller = TextEditingController(text: 'Ship it');
      addTearDown(controller.dispose);
      await tester.pumpWidget(_kit(brightness, controller));
      await tester.pumpAndSettle();
      await expectLater(find.byType(MaterialApp), matchesGoldenFile('kit_bubble_$name.png'));
    });
  }
}
