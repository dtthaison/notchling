import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/tokens.dart';
import '../../app/widgets/kit.dart';
import '../../app/widgets/motion.dart';
import '../../l10n/app_localizations.dart';
import '../macs/macs_screen.dart';
import 'app_settings.dart';

class SettingsScreen extends ConsumerWidget {
  const SettingsScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final AppL10n l10n = AppL10n.of(context);
    final Kit kit = Kit.of(context);
    final AppSettings settings = ref.watch(appSettingsProvider);
    return Scaffold(
      appBar: AppBar(
        toolbarHeight: 48,
        backgroundColor: kit.sidebar,
        title: Text(l10n.settingsTitle, style: kit.heading),
      ),
      body: SafeArea(
        child: ListView(
          padding: const EdgeInsets.all(Space.lg),
          children: <Widget>[
            Text(l10n.settingsTheme, style: kit.section),
            const SizedBox(height: Space.sm),
            ThemeChoice(
              mode: settings.themeMode,
              onChanged: ref.read(appSettingsProvider.notifier).setThemeMode,
            ),
            const SizedBox(height: Space.xl),
            Text(l10n.settingsNotifications, style: kit.section),
            const SizedBox(height: Space.sm),
            Surface(
              level: 1,
              child: Material(
                type: MaterialType.transparency,
                child: SwitchListTile(
                  value: settings.sound,
                  onChanged: (bool on) =>
                      ref.read(appSettingsProvider.notifier).setSound(on: on),
                  activeThumbColor: kit.onAccent,
                  activeTrackColor: kit.accent,
                  title: Text(l10n.settingsSound, style: kit.body),
                  subtitle: Text(l10n.settingsSoundHint, style: kit.meta),
                ),
              ),
            ),
            const SizedBox(height: Space.xl),
            Surface(
              level: 1,
              child: Material(
                type: MaterialType.transparency,
                child: ListTile(
                  leading: Icon(Icons.laptop_mac, color: kit.textMuted),
                  title: Text(l10n.openMacs, style: kit.body),
                  trailing: Icon(Icons.chevron_right, color: kit.textMuted),
                  onTap: () => unawaited(
                    Navigator.of(context).push(
                      MaterialPageRoute<void>(
                        builder: (BuildContext context) => const MacsScreen(),
                      ),
                    ),
                  ),
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class ThemeChoice extends StatelessWidget {
  const ThemeChoice({super.key, required this.mode, required this.onChanged});
  final ThemeMode mode;
  final void Function(ThemeMode) onChanged;

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final Kit kit = Kit.of(context);
    final List<(ThemeMode, String, IconData)> items =
        <(ThemeMode, String, IconData)>[
          (ThemeMode.system, l10n.themeSystem, Icons.brightness_auto_outlined),
          (ThemeMode.dark, l10n.themeDark, Icons.dark_mode_outlined),
          (ThemeMode.light, l10n.themeLight, Icons.light_mode_outlined),
        ];
    final int selected = items.indexWhere(
      ((ThemeMode, String, IconData) i) => i.$1 == mode,
    );
    return Semantics(
      container: true,
      label: l10n.settingsTheme,
      child: Surface(
        level: 0,
        fill: kit.raised,
        radius: BorderRadius.circular(Radii.md),
        padding: const EdgeInsets.all(4),
        child: LayoutBuilder(
          builder: (BuildContext context, BoxConstraints box) {
            final double width = box.maxWidth / items.length;
            return SizedBox(
              height: 44,
              child: Stack(
                children: <Widget>[
                  AnimatedPositioned(
                    duration: motionDuration(context, Motion.sheet),
                    curve: Curves.easeOutBack,
                    left: selected * width,
                    top: 0,
                    bottom: 0,
                    width: width,
                    child: Surface(
                      level: 1,
                      fill: kit.card,
                      radius: BorderRadius.circular(Radii.sm),
                      child: const SizedBox.expand(),
                    ),
                  ),
                  Row(
                    children: <Widget>[
                      for (final (ThemeMode, String, IconData) item in items)
                        Expanded(
                          child: Semantics(
                            button: true,
                            selected: item.$1 == mode,
                            label: item.$2,
                            excludeSemantics: true,
                            child: GestureDetector(
                              behavior: HitTestBehavior.opaque,
                              onTap: () => onChanged(item.$1),
                              child: Center(
                                child: Row(
                                  mainAxisSize: MainAxisSize.min,
                                  children: <Widget>[
                                    Icon(
                                      item.$3,
                                      size: 16,
                                      color: item.$1 == mode
                                          ? kit.accentText
                                          : kit.textMuted,
                                    ),
                                    const SizedBox(width: 6),
                                    Flexible(
                                      child: Text(
                                        item.$2,
                                        maxLines: 1,
                                        overflow: TextOverflow.ellipsis,
                                        style: kit.label.copyWith(
                                          color: item.$1 == mode
                                              ? kit.text
                                              : kit.textMuted,
                                        ),
                                      ),
                                    ),
                                  ],
                                ),
                              ),
                            ),
                          ),
                        ),
                    ],
                  ),
                ],
              ),
            );
          },
        ),
      ),
    );
  }
}
