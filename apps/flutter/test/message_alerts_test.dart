import 'package:agent_console/app/alerts.dart';
import 'package:agent_console/app/message_alerts.dart';
import 'package:agent_console/app/nav_state.dart';
import 'package:agent_console/app/providers.dart';
import 'package:agent_console/core/pairing/mac_store.dart';
import 'package:agent_console/app/notifications.dart';
import 'package:agent_console/core/net/bridge_client.dart';
import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/connection/connection.dart';
import 'package:agent_console/features/sessions/sessions_notifier.dart';
import 'package:agent_console/features/settings/app_settings.dart';
import 'package:fake_async/fake_async.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:flutter_test/flutter_test.dart';

Session _s(String id, {String? reply, SessionState state = SessionState.running, PendingPrompt? pending}) => Session(
      id: id,
      name: 'Session $id',
      project: 'p',
      cwd: '/p',
      entrypoint: 'cli',
      state: state,
      control: SessionControl.console,
      lastActivity: DateTime(2026, 11, 4),
      pending: pending,
      lastMessage: reply == null ? null : LastMessage(id: reply, text: 'Reply $reply\nsecond line'),
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

class _Sounds implements AlertSounds {
  final List<AlertKind> played = <AlertKind>[];
  @override
  Future<void> play(AlertKind kind) async => played.add(kind);
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  group('MessageAlertTracker', () {
    final DateTime t0 = DateTime(2026, 11, 4, 12);

    test('the first list is only recorded: an old reply is not news', () {
      final MessageAlertTracker tracker = MessageAlertTracker();
      expect(tracker.update(<Session>[_s('a', reply: 'm1')], now: t0), isEmpty);
    });

    test('a new reply is announced once, with its first line; the same reply again says nothing', () {
      final MessageAlertTracker tracker = MessageAlertTracker()..update(<Session>[_s('a', reply: 'm1')], now: t0);
      final List<MessageAlert> alerts = tracker.update(<Session>[_s('a', reply: 'm2')], now: t0.add(const Duration(seconds: 30)));
      expect(alerts.single.sessionId, 'a');
      expect(alerts.single.text, 'Reply m2');
      expect(alerts.single.quiet, isFalse);
      expect(tracker.update(<Session>[_s('a', reply: 'm2')], now: t0.add(const Duration(seconds: 31))), isEmpty);
      expect(tracker.update(<Session>[_s('a', reply: 'm2')], now: t0.add(const Duration(seconds: 32))), isEmpty);
    });

    test('a turn that starts and ends between two lists is still announced (the session never showed "running")', () {
      final MessageAlertTracker tracker = MessageAlertTracker()..update(<Session>[_s('a', reply: 'm1', state: SessionState.done)], now: t0);
      final List<MessageAlert> alerts = tracker.update(<Session>[_s('a', reply: 'm2', state: SessionState.done)], now: t0.add(const Duration(minutes: 1)));
      expect(alerts, hasLength(1));
    });

    test('a burst within 8 seconds updates in place, quietly; later ones are loud again', () {
      final MessageAlertTracker tracker = MessageAlertTracker()..update(<Session>[_s('a', reply: 'm1')], now: t0);
      expect(tracker.update(<Session>[_s('a', reply: 'm2')], now: t0.add(const Duration(seconds: 20))).single.quiet, isFalse);
      expect(tracker.update(<Session>[_s('a', reply: 'm3')], now: t0.add(const Duration(seconds: 23))).single.quiet, isTrue);
      expect(tracker.update(<Session>[_s('a', reply: 'm4')], now: t0.add(const Duration(seconds: 26))).single.quiet, isTrue);
      expect(tracker.update(<Session>[_s('a', reply: 'm5')], now: t0.add(const Duration(seconds: 40))).single.quiet, isFalse);
    });

    test('each session has its own window; a muted session is recorded but silent, and is not announced later for an old reply', () {
      final MessageAlertTracker tracker = MessageAlertTracker()..update(<Session>[_s('a', reply: 'a1'), _s('b', reply: 'b1')], now: t0);
      final List<MessageAlert> alerts = tracker.update(<Session>[_s('a', reply: 'a2'), _s('b', reply: 'b2')], now: t0.add(const Duration(seconds: 10)), muted: <String>{'b'});
      expect(alerts.map((MessageAlert m) => m.sessionId), <String>['a']);
      expect(tracker.update(<Session>[_s('a', reply: 'a2'), _s('b', reply: 'b2')], now: t0.add(const Duration(seconds: 60))), isEmpty);
    });

    test('an older Mac without lastMessage, and a session that just appeared, announce nothing', () {
      final MessageAlertTracker tracker = MessageAlertTracker()..update(<Session>[_s('a')], now: t0);
      expect(tracker.update(<Session>[_s('a'), _s('new', reply: 'n1')], now: t0.add(const Duration(seconds: 5))), isEmpty);
    });

    test('needs-you wins over a reply for the same session; a reply covers "finished"', () {
      final PendingPrompt prompt = const PendingPrompt(requestId: 'r', sessionId: 'a', isPermission: true, toolName: 'Bash', summary: 'git push', questions: <PromptQuestion>[]);
      final List<AlertEvent> both = buildAlertEvents(
        sessions: <Session>[_s('a', pending: prompt, state: SessionState.needsInput)],
        stateAlerts: <String, AlertKind>{'a': AlertKind.needsInput},
        messages: const <MessageAlert>[MessageAlert(sessionId: 'a', name: 'Session a', text: 'hi', quiet: false)],
        needsBody: 'Cần bạn',
        finishedBody: 'Xong',
      );
      expect(both.single.kind, AlertKind.needsInput);
      expect(both.single.body, 'Bash: git push');
    });
  });

  group('the watcher', () {
    late _Service service;
    late _Sounds sounds;

    Future<ProviderContainer> make({AppLifecycleState lifecycle = AppLifecycleState.resumed, bool sound = true}) async {
      service = _Service();
      sounds = _Sounds();
      final ProviderContainer c = ProviderContainer(overrides: <Override>[
        sessionsProvider.overrideWith(_Sessions.new),
        connectionStatusProvider.overrideWith(_Connected.new),
        notificationServiceProvider.overrideWithValue(service),
        alertSoundsProvider.overrideWithValue(sounds),
        settingsStoreProvider.overrideWithValue(MemorySettingsStore(AppSettings(sound: sound))),
        secretStoreProvider.overrideWithValue(MemorySecretStore()),
      ]);
      addTearDown(c.dispose);
      c.read(lifecycleProvider.notifier).set(lifecycle);
      c.listen<void>(attentionWatcherProvider, (void a, void b) {});
      c.listen<AlertEvent?>(inAppAlertProvider, (AlertEvent? a, AlertEvent? b) {});
      c.listen<AppSettings>(appSettingsProvider, (AppSettings? a, AppSettings b) {});
      c.listen<AlertPrefs>(alertPrefsProvider, (AlertPrefs? a, AlertPrefs b) {});
      await Future<void>.delayed(Duration.zero);
      return c;
    }

    void push(ProviderContainer c, List<Session> sessions) => (c.read(sessionsProvider.notifier) as _Sessions).push(sessions);

    test('app open: a reply in another session shows a banner with a sound and a haptic, and no system notification', () async {
      final ProviderContainer c = await make();
      push(c, <Session>[_s('a', reply: 'a1'), _s('b', reply: 'b1')]);
      push(c, <Session>[_s('a', reply: 'a1'), _s('b', reply: 'b2')]);
      expect(c.read(inAppAlertProvider)?.sessionId, 'b');
      expect(c.read(inAppAlertProvider)?.body, 'Reply b2');
      expect(sounds.played, <AlertKind>[AlertKind.message]);
      expect(service.shown, isEmpty);
    });

    test('app open: a reply in the chat that is on screen says nothing', () async {
      final ProviderContainer c = await make();
      c.read(activeTabProvider.notifier).show(1);
      c.read(selectedSessionIdProvider.notifier).select('a');
      push(c, <Session>[_s('a', reply: 'a1')]);
      push(c, <Session>[_s('a', reply: 'a2')]);
      expect(c.read(inAppAlertProvider), isNull);
      expect(sounds.played, isEmpty);
      c.read(activeTabProvider.notifier).show(0);
      push(c, <Session>[_s('a', reply: 'a3')]);
      expect(c.read(inAppAlertProvider)?.sessionId, 'a');
    });

    test('a quick second reply updates the banner without a second sound; sounds can be turned off', () async {
      final ProviderContainer c = await make();
      push(c, <Session>[_s('a', reply: 'a1')]);
      push(c, <Session>[_s('a', reply: 'a2')]);
      push(c, <Session>[_s('a', reply: 'a3')]);
      expect(c.read(inAppAlertProvider)?.body, 'Reply a3');
      expect(sounds.played, hasLength(1));
      final ProviderContainer silent = await make(sound: false);
      push(silent, <Session>[_s('a', reply: 'a1')]);
      push(silent, <Session>[_s('a', reply: 'a2')]);
      expect(silent.read(inAppAlertProvider), isNotNull, reason: 'the banner still shows');
      expect(sounds.played, isEmpty);
    });

    test('a muted session says nothing, in the app or in the background', () async {
      final ProviderContainer c = await make();
      await c.read(alertPrefsProvider.notifier).setMuted('a', muted: true);
      push(c, <Session>[_s('a', reply: 'a1')]);
      push(c, <Session>[_s('a', reply: 'a2')]);
      expect(c.read(inAppAlertProvider), isNull);
      expect(service.shown, isEmpty);
    });

    test('background: one system notification per session (tag = session), a needs-you for a permission, none for the first list', () async {
      final ProviderContainer c = await make(lifecycle: AppLifecycleState.paused);
      const PendingPrompt prompt = PendingPrompt(requestId: 'r', sessionId: 'b', isPermission: true, toolName: 'Bash', summary: 'git push', questions: <PromptQuestion>[]);
      push(c, <Session>[_s('a', reply: 'a1'), _s('b')]);
      expect(service.shown, isEmpty, reason: 'the first list is not news');
      push(c, <Session>[_s('a', reply: 'a2'), _s('b', pending: prompt, state: SessionState.needsInput)]);
      expect(service.shown.map((AlertEvent e) => '${e.sessionId}:${e.kind.name}').toSet(), <String>{'a:message', 'b:needsInput'});
      expect(service.shown.firstWhere((AlertEvent e) => e.sessionId == 'b').body, 'Bash: git push');
      expect(c.read(inAppAlertProvider), isNull);
    });
  });

  group('the in-app banner', () {
    test('hides after 4 seconds, and a newer alert restarts the time', () {
      fakeAsync((FakeAsync async) {
        final ProviderContainer c = ProviderContainer();
        c.listen<AlertEvent?>(inAppAlertProvider, (AlertEvent? a, AlertEvent? b) {});
        const AlertEvent first = AlertEvent(sessionId: 'a', name: 'A', body: 'one', kind: AlertKind.message);
        c.read(inAppAlertProvider.notifier).show(first);
        async.elapse(const Duration(seconds: 3));
        expect(c.read(inAppAlertProvider), isNotNull);
        c.read(inAppAlertProvider.notifier).show(const AlertEvent(sessionId: 'a', name: 'A', body: 'two', kind: AlertKind.message));
        async.elapse(const Duration(seconds: 3));
        expect(c.read(inAppAlertProvider)?.body, 'two', reason: 'the clock restarted');
        async.elapse(const Duration(seconds: 2));
        expect(c.read(inAppAlertProvider), isNull);
        c.dispose();
      });
    });
  });
}
