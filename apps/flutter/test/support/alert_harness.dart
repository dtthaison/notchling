// The alert watcher over two paired computers, with a stand-in session list so nothing connects.
import 'package:agent_console/app/alerts.dart';
import 'package:agent_console/app/message_alerts.dart';
import 'package:agent_console/app/notifications.dart';
import 'package:agent_console/app/providers.dart';
import 'package:agent_console/core/net/bridge_client.dart';
import 'package:agent_console/core/pairing/mac_profile.dart';
import 'package:agent_console/core/pairing/mac_store.dart';
import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/connection/connection.dart';
import 'package:agent_console/features/macs/macs_notifier.dart';
import 'package:agent_console/features/sessions/sessions_notifier.dart';
import 'package:agent_console/features/settings/app_settings.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:flutter_test/flutter_test.dart';

import 'harness.dart';
import 'two_computers.dart';

/// A session on whichever computer is in use, with an optional last reply.
Session alertSession(String id, {String? reply, SessionState state = SessionState.running}) => Session(
      id: id,
      name: 'Session $id',
      project: 'p',
      cwd: '/p',
      entrypoint: 'cli',
      state: state,
      control: SessionControl.console,
      lastActivity: DateTime(2026, 11, 8),
      lastMessage: reply == null ? null : LastMessage(id: reply, text: 'Reply $reply'),
    );

class StandInSessions extends SessionsNotifier {
  @override
  SessionsState build() => const SessionsState(phase: LoadPhase.ready);
  void push(List<Session> sessions) => state = SessionsState(phase: LoadPhase.ready, sessions: sessions);
}

class _Connected extends ConnectionStatusNotifier {
  @override
  ConnectionStatus build() => const ConnectionStatus(LinkPhase.connected);
}

/// Records the system notifications instead of showing them.
class RecordingNotifications extends NotificationService {
  final List<AlertEvent> shown = <AlertEvent>[];
  @override
  Future<void> showAlert(AlertEvent event) async => shown.add(event);
}

class _Silent implements AlertSounds {
  @override
  Future<void> play(AlertKind kind) async {}
}

/// Two paired computers, [active] in use, the watchers running. [secrets] starts the phone from stored state.
Future<({ProviderContainer container, RecordingNotifications service})> watchedComputers({
  required MacProfile active,
  AppLifecycleState lifecycle = AppLifecycleState.resumed,
  MemorySecretStore? secrets,
}) async {
  final RecordingNotifications service = RecordingNotifications();
  final MemorySecretStore store = secrets ??
      (MemorySecretStore()
        ..values['macs.v1'] = encodeProfiles(<MacProfile>[macA, macB])
        ..values['macs.active'] = active.id);
  final ProviderContainer container = ProviderContainer(overrides: <Override>[
    secretStoreProvider.overrideWithValue(store),
    sessionsProvider.overrideWith(StandInSessions.new),
    connectionStatusProvider.overrideWith(_Connected.new),
    notificationServiceProvider.overrideWithValue(service),
    alertSoundsProvider.overrideWithValue(_Silent()),
    settingsStoreProvider.overrideWithValue(MemorySettingsStore(const AppSettings(sound: false))),
  ]);
  addTearDown(container.dispose);
  container.read(macsProvider);
  await until(() => container.read(macsProvider).loaded);
  container.read(lifecycleProvider.notifier).set(lifecycle);
  container.listen<void>(attentionWatcherProvider, (void a, void b) {});
  container.listen<void>(notificationTapsProvider, (void a, void b) {});
  container.listen<AlertEvent?>(inAppAlertProvider, (AlertEvent? a, AlertEvent? b) {});
  container.listen<AppSettings>(appSettingsProvider, (AppSettings? a, AppSettings b) {});
  container.listen<AlertPrefs>(alertPrefsProvider, (AlertPrefs? a, AlertPrefs b) {});
  await until(() => container.read(alertPrefsProvider).loaded);
  return (container: container, service: service);
}

/// The session list of the computer in use, as the bridge would push it.
void pushSessions(ProviderContainer c, List<Session> sessions) => (c.read(sessionsProvider.notifier) as StandInSessions).push(sessions);
