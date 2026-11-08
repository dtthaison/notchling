// An alert belongs to the computer that raised it: a tap on its banner or notification opens that computer's
// session even when the user switched computers after the alert (decision 0012).
import 'package:agent_console/app/alerts.dart';
import 'package:agent_console/app/message_alerts.dart';
import 'package:agent_console/app/nav_state.dart';
import 'package:agent_console/app/notifications.dart';
import 'package:agent_console/app/providers.dart';
import 'package:agent_console/app/widgets/alert_banner.dart';
import 'package:agent_console/core/net/bridge_client.dart';
import 'package:agent_console/core/pairing/mac_profile.dart';
import 'package:agent_console/core/pairing/mac_store.dart';
import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/connection/connection.dart';
import 'package:agent_console/features/macs/macs_notifier.dart';
import 'package:agent_console/features/macs/session_ref.dart';
import 'package:agent_console/features/sessions/sessions_notifier.dart';
import 'package:agent_console/features/settings/app_settings.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:flutter_test/flutter_test.dart';

import 'support/harness.dart';

const MacProfile macA = MacProfile(
  id: 'aaaaaaaaaaaaaaaa',
  name: 'Mac A',
  hosts: <String>['10.0.0.1'],
  port: 47821,
  fingerprint: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  token: 'tok-a',
);

const MacProfile macB = MacProfile(
  id: 'bbbbbbbbbbbbbbbb',
  name: 'Linux B',
  hosts: <String>['10.0.0.2'],
  port: 47821,
  fingerprint: 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
  token: 'tok-b',
);

Session _s(String id, {String? reply, SessionState state = SessionState.running}) => Session(
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

class _Sessions extends SessionsNotifier {
  @override
  SessionsState build() => const SessionsState(phase: LoadPhase.ready);
  void push(List<Session> sessions) => state = SessionsState(phase: LoadPhase.ready, sessions: sessions);
}

class _Connected extends ConnectionStatusNotifier {
  @override
  ConnectionStatus build() => const ConnectionStatus(LinkPhase.connected);
}

class _Service extends NotificationService {
  final List<AlertEvent> shown = <AlertEvent>[];
  @override
  Future<void> showAlert(AlertEvent event) async => shown.add(event);
}

class _Silent implements AlertSounds {
  @override
  Future<void> play(AlertKind kind) async {}
}

/// Two paired computers, [active] in use. Sessions come from a stand-in list, so nothing connects.
Future<({ProviderContainer container, _Service service})> twoComputers({required MacProfile active, AppLifecycleState lifecycle = AppLifecycleState.resumed}) async {
  final _Service service = _Service();
  final MemorySecretStore secrets = MemorySecretStore()
    ..values['macs.v1'] = encodeProfiles(<MacProfile>[macA, macB])
    ..values['macs.active'] = active.id;
  final ProviderContainer container = ProviderContainer(overrides: <Override>[
    secretStoreProvider.overrideWithValue(secrets),
    sessionsProvider.overrideWith(_Sessions.new),
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

void _push(ProviderContainer c, List<Session> sessions) => (c.read(sessionsProvider.notifier) as _Sessions).push(sessions);

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  group('an alert says which computer raised it', () {
    test('a reply while B is in use carries B, in the banner and in the notification', () async {
      final (:ProviderContainer container, :_Service service) = await twoComputers(active: macB);
      _push(container, <Session>[_s('s1', reply: 'm1')]);
      _push(container, <Session>[_s('s1', reply: 'm2')]);
      expect(container.read(inAppAlertProvider)?.host, macB.id);
      expect(container.read(inAppAlertProvider)?.session, SessionRef(macB.id, 's1'));

      container.read(lifecycleProvider.notifier).set(AppLifecycleState.paused);
      _push(container, <Session>[_s('s1', reply: 'm3')]);
      expect(service.shown.single.host, macB.id);
      expect(alertPayload(service.shown.single), '${macB.id}/s1');
    });
  });

  group('tapping a notification', () {
    test('after switching to A, a notification from B opens B’s session and makes B the computer in use', () async {
      final (:ProviderContainer container, :_Service service) = await twoComputers(active: macB, lifecycle: AppLifecycleState.paused);
      _push(container, <Session>[_s('s1', reply: 'm1')]);
      _push(container, <Session>[_s('s1', reply: 'm2')]);
      final String payload = alertPayload(service.shown.single);

      await container.read(macsProvider.notifier).setActive(macA.id);
      service.receiveTap(payload);
      await until(() => container.read(selectedSessionProvider) != null);

      expect(container.read(selectedSessionProvider), SessionRef(macB.id, 's1'));
      expect(container.read(macsProvider).activeId, macB.id);
      expect(container.read(activeTabProvider), 1);
    });

    test('a payload from before computers had a host opens that session on the computer in use', () async {
      final (:ProviderContainer container, :_Service service) = await twoComputers(active: macA);
      service.receiveTap('s1');
      await until(() => container.read(selectedSessionProvider) != null);
      expect(container.read(selectedSessionProvider), SessionRef(macA.id, 's1'));
      expect(container.read(macsProvider).activeId, macA.id);
    });

    test('a session id with a slash or a colon survives the round trip', () async {
      final (:ProviderContainer container, :_Service service) = await twoComputers(active: macA);
      final AlertEvent event = AlertEvent(host: macB.id, sessionId: 'codex:019a/x', name: 'x', body: 'y', kind: AlertKind.message);
      service.receiveTap(alertPayload(event));
      await until(() => container.read(selectedSessionProvider) != null);
      expect(container.read(selectedSessionProvider), SessionRef(macB.id, 'codex:019a/x'));
    });
  });

  group('tapping the banner', () {
    testWidgets('after switching to A, the banner of an alert from B opens B’s session', (WidgetTester tester) async {
      final (:ProviderContainer container, service: _) = (await tester.runAsync(() => twoComputers(active: macB)))!;
      await tester.pumpWidget(UncontrolledProviderScope(container: container, child: host(const InAppAlertBanner(), screen: true)));
      container.read(inAppAlertProvider.notifier).show(AlertEvent(host: macB.id, sessionId: 's1', name: 'Session s1', body: 'Reply m2', kind: AlertKind.message));
      await tester.runAsync(() => container.read(macsProvider.notifier).setActive(macA.id));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 600));
      expect(find.text('Session s1'), findsOneWidget);

      await tester.tap(find.text('Session s1'));
      await tester.pump();

      expect(container.read(selectedSessionProvider), SessionRef(macB.id, 's1'));
      expect(container.read(macsProvider).activeId, macB.id);
      expect(container.read(activeTabProvider), 1);
      await tester.pump(const Duration(seconds: 1));
    });
  });
}
