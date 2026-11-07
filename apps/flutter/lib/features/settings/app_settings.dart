import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:path_provider/path_provider.dart';

/// Preferences of this phone: the theme and whether in-app sounds play. Stored as a small JSON file next to the
/// archive; a file that cannot be read gives the defaults (follow the system, sounds on).
@immutable
class AppSettings {
  const AppSettings({this.themeMode = ThemeMode.system, this.sound = true});
  final ThemeMode themeMode;
  final bool sound;

  AppSettings copyWith({ThemeMode? themeMode, bool? sound}) => AppSettings(themeMode: themeMode ?? this.themeMode, sound: sound ?? this.sound);

  Map<String, Object?> toJson() => <String, Object?>{'themeMode': themeMode.name, 'sound': sound};

  static AppSettings fromJson(Map<String, Object?> json) => AppSettings(
        themeMode: ThemeMode.values.firstWhere((ThemeMode m) => m.name == json['themeMode'], orElse: () => ThemeMode.system),
        sound: json['sound'] is bool ? json['sound']! as bool : true,
      );

  @override
  bool operator ==(Object other) => other is AppSettings && other.themeMode == themeMode && other.sound == sound;

  @override
  int get hashCode => Object.hash(themeMode, sound);
}

abstract interface class SettingsStore {
  Future<AppSettings> load();
  Future<void> save(AppSettings settings);
}

class DiskSettingsStore implements SettingsStore {
  DiskSettingsStore([Future<Directory> Function()? root]) : _root = root ?? getApplicationSupportDirectory;
  final Future<Directory> Function() _root;

  Future<File> _file() async => File('${(await _root()).path}/settings.json');

  @override
  Future<AppSettings> load() async {
    try {
      final File file = await _file();
      if (!file.existsSync()) return const AppSettings();
      final Object? decoded = jsonDecode(await file.readAsString());
      return decoded is Map<String, Object?> ? AppSettings.fromJson(decoded) : const AppSettings();
    } on Object catch (error) {
      debugPrint('SettingsStore.load: $error');
      return const AppSettings();
    }
  }

  @override
  Future<void> save(AppSettings settings) async {
    try {
      final File file = await _file();
      await file.parent.create(recursive: true);
      final File scratch = File('${file.path}.tmp');
      await scratch.writeAsString(jsonEncode(settings.toJson()), flush: true);
      await scratch.rename(file.path);
    } on Object catch (error) {
      debugPrint('SettingsStore.save: $error');
    }
  }
}

class MemorySettingsStore implements SettingsStore {
  MemorySettingsStore([this.saved = const AppSettings()]);
  AppSettings saved;

  @override
  Future<AppSettings> load() async => saved;

  @override
  Future<void> save(AppSettings settings) async => saved = settings;
}

final Provider<SettingsStore> settingsStoreProvider = Provider<SettingsStore>((Ref ref) => DiskSettingsStore());

final NotifierProvider<SettingsNotifier, AppSettings> appSettingsProvider = NotifierProvider<SettingsNotifier, AppSettings>(SettingsNotifier.new);

class SettingsNotifier extends Notifier<AppSettings> {
  bool _touched = false;

  @override
  AppSettings build() {
    // Read the store now: the Ref may be gone by the time the file is read (a test, a hot restart).
    final SettingsStore store = ref.read(settingsStoreProvider);
    Future<void>(() async {
      final AppSettings saved = await store.load();
      // A choice made before the file was read wins over what was stored.
      if (ref.mounted && !_touched) state = saved;
    });
    return const AppSettings();
  }

  void setThemeMode(ThemeMode mode) => _set(state.copyWith(themeMode: mode));
  void setSound({required bool on}) => _set(state.copyWith(sound: on));

  void _set(AppSettings next) {
    _touched = true;
    state = next;
    ref.read(settingsStoreProvider).save(next);
  }
}
