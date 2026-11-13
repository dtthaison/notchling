import 'package:agent_console/app/alerts.dart';
import 'package:agent_console/app/message_alerts.dart';
import 'package:agent_console/app/nav_state.dart';
import 'package:agent_console/app/notifications.dart';
import 'package:agent_console/app/widgets/alert_banner.dart';
import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/macs/macs_notifier.dart';
import 'package:agent_console/features/macs/session_ref.dart';
import 'package:agent_console/features/sessions/sessions_notifier.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/alert_harness.dart';
import 'support/harness.dart';
import 'support/two_computers.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  group('an alert says which computer raised it', () {
    test('a reply while B is in use carries B, in the banner and in the notification', () async {
      final (:ProviderContainer container, :RecordingNotifications service) = await watchedComputers(active: macB);
      pushSessions(container, <Session>[alertSession('s1', reply: 'm1')]);
      pushSessions(container, <Session>[alertSession('s1', reply: 'm2')]);
      expect(container.read(inAppAlertProvider)?.host, macB.id);
      expect(container.read(inAppAlertProvider)?.session, SessionRef(macB.id, 's1'));

      container.read(lifecycleProvider.notifier).set(AppLifecycleState.paused);
      pushSessions(container, <Session>[alertSession('s1', reply: 'm3')]);
      expect(service.shown.single.host, macB.id);
      expect(alertPayload(service.shown.single), '${macB.id}/s1');
    });
  });

  group('tapping a notification', () {
    test('after switching to A, a notification from B opens B’s session and makes B the computer in use', () async {
      final (:ProviderContainer container, :RecordingNotifications service) = await watchedComputers(active: macB, lifecycle: AppLifecycleState.paused);
      pushSessions(container, <Session>[alertSession('s1', reply: 'm1')]);
      pushSessions(container, <Session>[alertSession('s1', reply: 'm2')]);
      final String payload = alertPayload(service.shown.single);

      await container.read(macsProvider.notifier).setActive(macA.id);
      service.receiveTap(payload);
      await until(() => container.read(selectedSessionProvider) != null);

      expect(container.read(selectedSessionProvider), SessionRef(macB.id, 's1'));
      expect(container.read(macsProvider).activeId, macB.id);
      expect(container.read(activeTabProvider), 1);
    });

    test('a payload from before computers had a host opens that session on the computer in use', () async {
      final (:ProviderContainer container, :RecordingNotifications service) = await watchedComputers(active: macA);
      service.receiveTap('s1');
      await until(() => container.read(selectedSessionProvider) != null);
      expect(container.read(selectedSessionProvider), SessionRef(macA.id, 's1'));
      expect(container.read(macsProvider).activeId, macA.id);
    });

    test('a session id with a slash or a colon survives the round trip', () async {
      final (:ProviderContainer container, :RecordingNotifications service) = await watchedComputers(active: macA);
      const AlertEvent event = AlertEvent(host: 'bbbbbbbbbbbbbbbb', sessionId: 'codex:019a/x', name: 'x', body: 'y', kind: AlertKind.message);
      service.receiveTap(alertPayload(event));
      await until(() => container.read(selectedSessionProvider) != null);
      expect(container.read(selectedSessionProvider), SessionRef(macB.id, 'codex:019a/x'));
    });
  });

  group('tapping the banner', () {
    testWidgets('after switching to A, the banner of an alert from B opens B’s session', (WidgetTester tester) async {
      final (:ProviderContainer container, service: _) = (await tester.runAsync(() => watchedComputers(active: macB)))!;
      await tester.pumpWidget(UncontrolledProviderScope(container: container, child: host(const InAppAlertBanner(), screen: true)));
      container.read(inAppAlertProvider.notifier).show(const AlertEvent(host: 'bbbbbbbbbbbbbbbb', sessionId: 's1', name: 'Session s1', body: 'Reply m2', kind: AlertKind.message));
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
