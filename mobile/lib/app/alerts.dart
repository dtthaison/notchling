import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../core/pairing/mac_store.dart';
import '../core/protocol/models.dart';
import '../features/macs/macs_notifier.dart';
import '../features/sessions/sessions_notifier.dart';
import 'providers.dart';

/// What changed in a session that is worth a notification.
enum AlertKind { needsInput, finished }

/// Alerts between two session lists, once per change: a session that has just gone to "Cần input", or from
/// running to done. [announced] remembers what each session was last announced as, so repeated lists carrying
/// the same state, or a state that flips back and forth between two polls, do not raise the same alert twice.
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
  // A session that moved on (running again, say) may be announced again for its next change.
  for (final Session s in after) {
    final AlertKind? last = announced[s.id];
    if (last == AlertKind.needsInput && s.state != SessionState.needsInput) announced.remove(s.id);
    if (last == AlertKind.finished && s.state != SessionState.done) announced.remove(s.id);
  }
  announced.addAll(result);
  return result;
}

/// The user's choices about background behaviour and which sessions are muted, kept in the secure store.
class AlertPrefs {
  const AlertPrefs({this.keepAlive, this.muted = const <String>{}, this.loaded = false});

  /// Null until the user has answered the first-run question.
  final bool? keepAlive;
  final Set<String> muted;
  final bool loaded;

  AlertPrefs copyWith({bool? keepAlive, Set<String>? muted, bool? loaded}) =>
      AlertPrefs(keepAlive: keepAlive ?? this.keepAlive, muted: muted ?? this.muted, loaded: loaded ?? this.loaded);
}

final NotifierProvider<AlertPrefsNotifier, AlertPrefs> alertPrefsProvider = NotifierProvider<AlertPrefsNotifier, AlertPrefs>(AlertPrefsNotifier.new);

class AlertPrefsNotifier extends Notifier<AlertPrefs> {
  static const String _keepAliveKey = 'prefs.keepalive';
  static const String _mutedKey = 'prefs.muted';

  @override
  AlertPrefs build() {
    unawaited(_load());
    return const AlertPrefs();
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

  Future<void> setMuted(String sessionId, {required bool muted}) async {
    final Set<String> next = <String>{...state.muted};
    muted ? next.add(sessionId) : next.remove(sessionId);
    state = state.copyWith(muted: next);
    await _store.write(_mutedKey, jsonEncode(next.toList()));
  }
}

/// The Android foreground service that keeps the app, and so the Mac connection, alive in the background.
/// The service is native (MainActivity / KeepAliveService); this only starts, updates and stops it.
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
    } on PlatformException {
      // The service could not start (notifications blocked, say); the app just works as before.
    }
  }

  @override
  Future<void> stop() async {
    try {
      await _channel.invokeMethod<void>('stop');
    } on PlatformException {
      // Already stopped.
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

/// Keeps the foreground service in step with the setting, the active Mac and the number of sessions.
final Provider<void> keepAliveControllerProvider = Provider<void>((Ref ref) {
  final bool on = ref.watch(alertPrefsProvider.select((AlertPrefs p) => p.keepAlive == true));
  final bool hasMac = ref.watch(macsProvider.select((MacsState s) => s.active != null));
  final int count = ref.watch(sessionsProvider.select((SessionsState s) => s.sessions.where((Session x) => x.state == SessionState.running || x.state == SessionState.needsInput).length));
  final BackgroundKeeper service = ref.read(keepAliveProvider);
  if (on && hasMac) {
    unawaited(service.start(count == 0 ? 'Agent Console đang theo dõi' : 'Agent Console đang theo dõi $count session'));
  } else {
    unawaited(service.stop());
  }
});
