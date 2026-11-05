import 'dart:async';

import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../core/protocol/models.dart';
import 'alerts.dart';

/// Claude said something new in a session. [text] is its first line. [quiet] is true when the same session was
/// announced a moment ago: the alert then only updates what is already showing (no sound, no new heads-up).
class MessageAlert {
  const MessageAlert({required this.sessionId, required this.name, required this.text, required this.quiet});
  final String sessionId;
  final String name;
  final String text;
  final bool quiet;
}

/// Finds new replies by comparing each session's `lastMessage.id` with the one seen before. It does not depend on the
/// session ever showing "running" in a list, so a turn that starts and ends between two pushes is still announced; and
/// the same reply is never announced twice, however often the list repeats it.
class MessageAlertTracker {
  MessageAlertTracker({this.window = const Duration(seconds: 8)});

  /// A session announced within this long ago is updated in place, quietly.
  final Duration window;
  final Map<String, String> _seen = <String, String>{};
  final Map<String, DateTime> _lastLoud = <String, DateTime>{};
  bool _primed = false;

  /// The first list only records what is there (an old reply is not news). Muted sessions are recorded but not
  /// announced. Deciding whether to show an alert for the chat that is open is the caller's.
  List<MessageAlert> update(List<Session> sessions, {required DateTime now, Set<String> muted = const <String>{}}) {
    final List<MessageAlert> alerts = <MessageAlert>[];
    for (final Session s in sessions) {
      final LastMessage? last = s.lastMessage;
      if (last == null) continue;
      final String? seen = _seen[s.id];
      _seen[s.id] = last.id;
      if (!_primed || seen == null || seen == last.id) continue;
      if (muted.contains(s.id)) continue;
      final DateTime? before = _lastLoud[s.id];
      final bool quiet = before != null && now.difference(before) < window;
      if (!quiet) _lastLoud[s.id] = now;
      alerts.add(MessageAlert(sessionId: s.id, name: s.name, text: firstLine(last.text), quiet: quiet));
    }
    _primed = true;
    return alerts;
  }

  static String firstLine(String text) {
    final String line = text.split('\n').map((String l) => l.trim()).firstWhere((String l) => l.isNotEmpty, orElse: () => '');
    return line.length > 140 ? '${line.substring(0, 139)}…' : line;
  }
}

/// What the app announces: a session that needs the user, a new reply, or a finished turn.
class AlertEvent {
  const AlertEvent({required this.sessionId, required this.name, required this.body, required this.kind, this.quiet = false});
  final String sessionId;
  final String name;
  final String body;
  final AlertKind kind;
  final bool quiet;
}

/// One event per session, the most important first: needs you, then a new reply, then finished.
List<AlertEvent> buildAlertEvents({
  required List<Session> sessions,
  required Map<String, AlertKind> stateAlerts,
  required List<MessageAlert> messages,
  required String needsBody,
  required String finishedBody,
}) {
  final Map<String, AlertEvent> bySession = <String, AlertEvent>{};
  for (final MapEntry<String, AlertKind> entry in stateAlerts.entries) {
    final Session? s = sessions.where((Session x) => x.id == entry.key).firstOrNull;
    if (s == null) continue;
    final PendingPrompt? prompt = s.pending;
    final String summary = prompt == null ? '' : (prompt.isPermission ? '${prompt.toolName}: ${prompt.summary}' : (prompt.questions.isNotEmpty ? prompt.questions.first.question : prompt.summary));
    bySession[s.id] = AlertEvent(
      sessionId: s.id,
      name: s.name,
      kind: entry.value,
      body: entry.value == AlertKind.needsInput ? (summary.isEmpty ? needsBody : summary.replaceAll('\n', ' ')) : finishedBody,
    );
  }
  for (final MessageAlert m in messages) {
    final AlertEvent? existing = bySession[m.sessionId];
    if (existing != null && existing.kind == AlertKind.needsInput) continue;
    bySession[m.sessionId] = AlertEvent(sessionId: m.sessionId, name: m.name, kind: AlertKind.message, body: m.text, quiet: m.quiet);
  }
  return bySession.values.toList();
}

/// The banner shown inside the app. Auto-hides after 4 s; a newer alert for any session replaces it (and for the same
/// session it simply updates the text in place).
final NotifierProvider<InAppAlertNotifier, AlertEvent?> inAppAlertProvider = NotifierProvider<InAppAlertNotifier, AlertEvent?>(InAppAlertNotifier.new);

class InAppAlertNotifier extends Notifier<AlertEvent?> {
  Timer? _hide;
  static const Duration showFor = Duration(seconds: 4);

  @override
  AlertEvent? build() {
    ref.onDispose(() => _hide?.cancel());
    return null;
  }

  void show(AlertEvent event) {
    state = event;
    _hide?.cancel();
    _hide = Timer(showFor, dismiss);
  }

  void dismiss() {
    _hide?.cancel();
    _hide = null;
    if (state != null) state = null;
  }
}

/// A short sound for an alert while the app is open. Android plays the res/raw sound unless the ringer is not normal or
/// Do Not Disturb is on; elsewhere the system's alert sound.
abstract interface class AlertSounds {
  Future<void> play(AlertKind kind);
}

class PlatformAlertSounds implements AlertSounds {
  static const MethodChannel _channel = MethodChannel('local.agentconsole/sounds');

  @override
  Future<void> play(AlertKind kind) async {
    try {
      await _channel.invokeMethod<void>('play', <String, Object?>{'name': kind == AlertKind.needsInput ? 'needs_you' : 'new_message'});
    } on MissingPluginException {
      await SystemSound.play(SystemSoundType.alert);
    } on PlatformException {
      // No sound is not an error: the banner shows.
    }
  }
}

final Provider<AlertSounds> alertSoundsProvider = Provider<AlertSounds>((Ref ref) => PlatformAlertSounds());
