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

/// What changed in a session that is worth a notification.
enum AlertKind { needsInput, finished, message }

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

  /// What is muted is kept by [storageKey], like the chat cache and the archive: a bare id belongs to the computer
  /// that kept the empty scope (the one a phone had before it followed several), `<scope>/<id>` to any other. So
  /// muting `s1` on one computer leaves `s1` on another heard.
  /// Whether [sessionId] on the computer whose storage scope is [scope] is muted.
  bool isMuted(String scope, String sessionId) => muted.contains(storageKey(scope, sessionId));

  /// The ids (as that computer gave them) muted on the computer whose storage scope is [scope].
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

  /// Mutes or unmutes [sessionId] on the computer whose storage scope is [scope] (the empty scope by default,
  /// which is a phone that follows one computer).
  Future<void> setMuted(String sessionId, {required bool muted, String scope = ''}) async {
    final Set<String> next = <String>{...state.muted};
    final String key = storageKey(scope, sessionId);
    muted ? next.add(key) : next.remove(key);
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
    unawaited(service.start(count == 0 ? 'Đang theo dõi' : 'Đang theo dõi · $count session'));
  } else {
    unawaited(service.stop());
  }
});
