import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../core/pairing/mac_store.dart';
import '../core/protocol/models.dart';
import '../features/macs/macs_notifier.dart';
import '../features/macs/session_ref.dart';
import '../features/sessions/sessions_notifier.dart';
import 'providers.dart';

enum AlertKind { needsInput, finished, message }

Map<String, AlertKind> alertsBetween(List<Session> before, List<Session> after, Map<String, AlertKind> announced) {
  final Map<String, SessionState> was = <String, SessionState>{for (final Session s in before) s.id: s.state};
  final Map<String, AlertKind> result = <String, AlertKind>{};
  for (final Session s in after) {
    final SessionState? previous = was[s.id];
    if (previous == null) continue;
    AlertKind? kind;
    if (s.state == SessionState.needsInput && previous != SessionState.needsInput) {
      kind = AlertKind.needsInput;
    } else if (s.state == SessionState.done && previous == SessionState.running) {
      kind = AlertKind.finished;
    }
    if (kind != null && announced[s.id] != kind) result[s.id] = kind;
  }
  for (final Session s in after) {
    final AlertKind? last = announced[s.id];
    if (last == AlertKind.needsInput && s.state != SessionState.needsInput) announced.remove(s.id);
    if (last == AlertKind.finished && s.state != SessionState.done) announced.remove(s.id);
  }
  announced.addAll(result);
  return result;
}

class AlertPrefs {
  const AlertPrefs({this.keepAlive, this.muted = const <String>{}, this.loaded = false});

  final bool? keepAlive;
  final Set<String> muted;
  final bool loaded;

  bool isMuted(String scope, String sessionId) => muted.contains(storageKey(scope, sessionId));

  Set<String> mutedIn(String scope) => scope.isEmpty
      ? <String>{for (final String key in muted) if (SessionRef.parse(key) == null) key}
      : <String>{for (final String key in muted) if (key.startsWith('$scope/')) key.substring(scope.length + 1)};

  AlertPrefs copyWith({bool? keepAlive, Set<String>? muted, bool? loaded}) =>
      AlertPrefs(keepAlive: keepAlive ?? this.keepAlive, muted: muted ?? this.muted, loaded: loaded ?? this.loaded);
}

final NotifierProvider<AlertPrefsNotifier, AlertPrefs> alertPrefsProvider = NotifierProvider<AlertPrefsNotifier, AlertPrefs>(AlertPrefsNotifier.new);

class AlertPrefsNotifier extends Notifier<AlertPrefs> {
  static const String _keepAliveKey = 'prefs.keepalive';
  static const String _mutedKey = 'prefs.muted';

  Future<void>? _loading;

  @override
  AlertPrefs build() {
    _loading = _load();
    return const AlertPrefs();
  }

  Future<void> removeScope(String scope) async {
    await _loading;
    if (!ref.mounted) return;
    final Set<String> kept = <String>{for (final String key in state.muted) if (!keyInScope(scope, key)) key};
    if (kept.length == state.muted.length) return;
    state = state.copyWith(muted: kept);
    await _store.write(_mutedKey, jsonEncode(kept.toList()));
  }

  SecretStore get _store => ref.read(secretStoreProvider);

  Future<void> _load() async {
    final String? keep = await _store.read(_keepAliveKey);
    final String? muted = await _store.read(_mutedKey);
    if (!ref.mounted) return;
    state = AlertPrefs(
      keepAlive: keep == null ? null : keep == 'on',
      muted: muted == null ? <String>{} : (jsonDecode(muted) as List<Object?>).whereType<String>().toSet(),
      loaded: true,
    );
  }

  Future<void> setKeepAlive({required bool on}) async {
    state = state.copyWith(keepAlive: on);
    await _store.write(_keepAliveKey, on ? 'on' : 'off');
  }

  Future<void> setMuted(String sessionId, {required bool muted, String scope = ''}) async {
    final Set<String> next = <String>{...state.muted};
    final String key = storageKey(scope, sessionId);
    muted ? next.add(key) : next.remove(key);
    state = state.copyWith(muted: next);
    await _store.write(_mutedKey, jsonEncode(next.toList()));
  }
}

abstract class BackgroundKeeper {
  Future<void> start(String text);
  Future<void> stop();
}

class AndroidBackgroundKeeper implements BackgroundKeeper {
  static const MethodChannel _channel = MethodChannel('local.agentconsole/keepalive');

  @override
  Future<void> start(String text) async {
    try {
      await _channel.invokeMethod<void>('start', <String, Object?>{'text': text});
    } on PlatformException catch (_) {
    }
  }

  @override
  Future<void> stop() async {
    try {
      await _channel.invokeMethod<void>('stop');
    } on PlatformException catch (_) {
    }
  }
}

class NoBackgroundKeeper implements BackgroundKeeper {
  @override
  Future<void> start(String text) async {}
  @override
  Future<void> stop() async {}
}

final Provider<BackgroundKeeper> keepAliveProvider = Provider<BackgroundKeeper>((Ref ref) => Platform.isAndroid ? AndroidBackgroundKeeper() : NoBackgroundKeeper());

final Provider<void> keepAliveControllerProvider = Provider<void>((Ref ref) {
  final bool on = ref.watch(alertPrefsProvider.select((AlertPrefs p) => p.keepAlive == true));
  final bool hasMac = ref.watch(macsProvider.select((MacsState s) => s.active != null));
  final int count = ref.watch(sessionsProvider.select((SessionsState s) => s.sessions.where((Session x) => x.state == SessionState.running || x.state == SessionState.needsInput).length));
  final BackgroundKeeper service = ref.read(keepAliveProvider);
  if (on && hasMac) {
    unawaited(service.start(count == 0 ? 'Đang theo dõi' : 'Đang theo dõi · $count session'));
  } else {
    unawaited(service.stop());
  }
});
