import 'package:agent_console/app/theme/tokens.dart';
import 'package:agent_console/features/settings/app_settings.dart';
import 'package:agent_console/features/settings/settings_screen.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:flutter_test/flutter_test.dart';

import 'support/harness.dart';

void main() {
  group('settings', () {
    test('the theme and the sound switch are saved, and a new start reads them back; the default follows the system', () async {
      final MemorySettingsStore store = MemorySettingsStore();
      final ProviderContainer first = ProviderContainer(overrides: <Override>[settingsStoreProvider.overrideWithValue(store)]);
      addTearDown(first.dispose);
      first.listen<AppSettings>(appSettingsProvider, (AppSettings? a, AppSettings b) {});
      expect(first.read(appSettingsProvider).themeMode, ThemeMode.system);
      expect(first.read(appSettingsProvider).sound, isTrue);
      first.read(appSettingsProvider.notifier).setThemeMode(ThemeMode.light);
      first.read(appSettingsProvider.notifier).setSound(on: false);
      await Future<void>.delayed(Duration.zero);
      expect(store.saved.themeMode, ThemeMode.light);
      expect(store.saved.sound, isFalse);

      final ProviderContainer second = ProviderContainer(overrides: <Override>[settingsStoreProvider.overrideWithValue(store)]);
      addTearDown(second.dispose);
      second.listen<AppSettings>(appSettingsProvider, (AppSettings? a, AppSettings b) {});
      await until(() => second.read(appSettingsProvider).themeMode == ThemeMode.light);
      expect(second.read(appSettingsProvider).sound, isFalse);
    });

    test('a file that is not settings gives the defaults', () {
      expect(AppSettings.fromJson(<String, Object?>{'themeMode': 'neon', 'sound': 'yes'}), const AppSettings());
    });

    testWidgets('the Settings screen offers System, Dark and Light, and choosing one applies it at once', (WidgetTester tester) async {
      final MemorySettingsStore store = MemorySettingsStore();
      final ProviderContainer container = ProviderContainer(overrides: <Override>[settingsStoreProvider.overrideWithValue(store)]);
      addTearDown(container.dispose);
      await tester.pumpWidget(UncontrolledProviderScope(container: container, child: host(const SettingsScreen(), screen: true)));
      await tester.pump();
      expect(find.text('System'), findsOneWidget);
      expect(find.text('Dark'), findsOneWidget);
      expect(find.text('Light'), findsOneWidget);
      await tester.tap(find.text('Light'));
      await tester.pumpAndSettle();
      expect(container.read(appSettingsProvider).themeMode, ThemeMode.light);
      expect(store.saved.themeMode, ThemeMode.light, reason: 'saved as soon as it is chosen');
      await tester.tap(find.text('Dark'));
      await tester.pumpAndSettle();
      expect(container.read(appSettingsProvider).themeMode, ThemeMode.dark);
    });

    test('the two themes blend colour by colour while the theme changes, and snap to the target at the end', () {
      final Kit mid = Kit.dark.lerp(Kit.light, 0.5);
      expect(mid.window, Color.lerp(Kit.dark.window, Kit.light.window, 0.5));
      expect(Kit.dark.lerp(Kit.light, 1).window, Kit.light.window);
      expect(Kit.dark.lerp(Kit.light, 0).text, Kit.dark.text);
    });
  });
}
