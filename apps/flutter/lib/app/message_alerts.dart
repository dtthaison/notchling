import 'dart:async';

import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../core/protocol/models.dart';
import '../features/macs/session_ref.dart';
import 'alerts.dart';

class MessageAlert {
  const MessageAlert({required this.sessionId, required this.name, required this.text, required this.quiet});
  final String sessionId;
  final String name;
  final String text;
  final bool quiet;
}

class MessageAlertTracker {
  MessageAlertTracker({this.window = const Duration(seconds: 8)});

  final Duration window;
  final Map<String, String> _seen = <String, String>{};
  final Map<String, DateTime> _lastLoud = <String, DateTime>{};
  bool _primed = false;

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

class AlertEvent {
  const AlertEvent({required this.sessionId, required this.name, required this.body, required this.kind, this.quiet = false, this.host = ''});

  final String host;
  final String sessionId;

  SessionRef get session => SessionRef(host, sessionId);

  final String name;
  final String body;
  final AlertKind kind;
  final bool quiet;
}

List<AlertEvent> buildAlertEvents({
  required List<Session> sessions,
  required Map<String, AlertKind> stateAlerts,
  required List<MessageAlert> messages,
  required String needsBody,
  required String finishedBody,
  String host = '',
}) {
  final Map<String, AlertEvent> bySession = <String, AlertEvent>{};
  for (final MapEntry<String, AlertKind> entry in stateAlerts.entries) {
    final Session? s = sessions.where((Session x) => x.id == entry.key).firstOrNull;
    if (s == null) continue;
    final PendingPrompt? prompt = s.pending;
    final String summary = prompt == null ? '' : (prompt.isPermission ? '${prompt.toolName}: ${prompt.summary}' : (prompt.questions.isNotEmpty ? prompt.questions.first.question : prompt.summary));
    bySession[s.id] = AlertEvent(
      host: host,
      sessionId: s.id,
      name: s.name,
      kind: entry.value,
      body: entry.value == AlertKind.needsInput ? (summary.isEmpty ? needsBody : summary.replaceAll('\n', ' ')) : finishedBody,
    );
  }
  for (final MessageAlert m in messages) {
    final AlertEvent? existing = bySession[m.sessionId];
    if (existing != null && existing.kind == AlertKind.needsInput) continue;
    bySession[m.sessionId] = AlertEvent(host: host, sessionId: m.sessionId, name: m.name, kind: AlertKind.message, body: m.text, quiet: m.quiet);
  }
  return bySession.values.toList();
}

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
    } on PlatformException catch (_) {
    }
  }
}

final Provider<AlertSounds> alertSoundsProvider = Provider<AlertSounds>((Ref ref) => PlatformAlertSounds());
