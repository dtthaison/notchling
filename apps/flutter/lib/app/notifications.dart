import 'dart:async';

import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_local_notifications/flutter_local_notifications.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../core/net/bridge_client.dart';
import '../core/protocol/models.dart';
import '../features/connection/connection.dart';
import '../features/macs/macs_notifier.dart';
import '../features/inbox/inbox_alerts.dart';
import '../features/inbox/inbox_notifier.dart';
import '../features/macs/session_ref.dart';
import '../features/sessions/sessions_notifier.dart';
import '../l10n/app_localizations.dart';
import 'alerts.dart';
import 'l10n_holder.dart';
import 'message_alerts.dart';
import 'nav_state.dart';
import '../features/settings/app_settings.dart';

class NotificationService implements InboxAlertSink {
  NotificationService([FlutterLocalNotificationsPlugin? plugin]) : _plugin = plugin ?? FlutterLocalNotificationsPlugin();
  final FlutterLocalNotificationsPlugin _plugin;
  final StreamController<String> _taps = StreamController<String>.broadcast();
  bool _ready = false;

  Stream<String> get taps => _taps.stream;

  void receiveTap(String payload) => _taps.add(payload);

  Future<void> init() async {
    if (_ready) return;
    await _plugin.initialize(
      settings: const InitializationSettings(
        android: AndroidInitializationSettings('@mipmap/ic_launcher'),
        iOS: DarwinInitializationSettings(requestAlertPermission: false, requestBadgePermission: false, requestSoundPermission: false),
      ),
      onDidReceiveNotificationResponse: (NotificationResponse response) {
        final String? payload = response.payload;
        if (payload != null) receiveTap(payload);
      },
    );
    _ready = true;
    final String? launched = await launchSession();
    if (launched != null) Future<void>.delayed(const Duration(milliseconds: 800), () => _taps.add(launched));
  }

  Future<void> requestPermission() async {
    if (!_ready) return;
    await _plugin.resolvePlatformSpecificImplementation<IOSFlutterLocalNotificationsPlugin>()?.requestPermissions(alert: true);
    await _plugin.resolvePlatformSpecificImplementation<AndroidFlutterLocalNotificationsPlugin>()?.requestNotificationsPermission();
  }

  Future<void> showAlert(AlertEvent event) async {
    if (!_ready) return;
    final AppL10n l10n = L10nHolder.current;
    final bool needs = event.kind == AlertKind.needsInput;
    final AndroidNotificationDetails android = AndroidNotificationDetails(
      needs ? 'needs_you_v2' : (event.kind == AlertKind.finished ? 'finished_v2' : 'messages_v2'),
      needs
          ? l10n.notificationsChannelNeedsYou
          : (event.kind == AlertKind.finished ? l10n.notificationsChannelFinished : l10n.notificationsChannelMessages),
      importance: needs ? Importance.high : Importance.defaultImportance,
      priority: needs ? Priority.high : Priority.defaultPriority,
      sound: event.kind == AlertKind.finished ? null : RawResourceAndroidNotificationSound(needs ? 'needs_you' : 'new_message'),
      playSound: event.kind != AlertKind.finished,
      tag: alertPayload(event),
      groupKey: _group,
      onlyAlertOnce: event.quiet,
      styleInformation: BigTextStyleInformation(event.body),
      category: AndroidNotificationCategory.message,
    );
    await _plugin.show(
      id: alertPayload(event).hashCode & 0x7fffffff,
      title: event.name,
      body: event.body,
      notificationDetails: NotificationDetails(android: android, iOS: DarwinNotificationDetails(threadIdentifier: _group, presentSound: !event.quiet)),
      payload: alertPayload(event),
    );
    await _plugin.show(
      id: 1,
      title: l10n.appName,
      body: l10n.notificationsSummaryBody,
      notificationDetails: NotificationDetails(
        android: AndroidNotificationDetails('messages_v2', l10n.notificationsChannelMessages, groupKey: _group, setAsGroupSummary: true, onlyAlertOnce: true, playSound: false, importance: Importance.low, priority: Priority.low),
        iOS: DarwinNotificationDetails(threadIdentifier: _group),
      ),
    );
  }

  static const String _group = 'sessions';

  static int _inboxId(String itemId) => 'inbox:$itemId'.hashCode & 0x7fffffff;

  @override
  Future<void> showInboxAlert(InboxAlert alert) async {
    if (!_ready) return;
    final AppL10n l10n = L10nHolder.current;
    final String body = switch (alert.rung) {
      InboxRung.first => alert.item.detail.isEmpty ? l10n.notificationsNeedsBody : alert.item.detail,
      InboxRung.stronger => l10n.notificationsWaitingStill,
      InboxRung.lastCall => l10n.notificationsWaitingLastCall,
    };
    final AndroidNotificationDetails android = AndroidNotificationDetails(
      'needs_you_v3',
      l10n.notificationsChannelWaiting,
      importance: Importance.high,
      priority: Priority.high,
      sound: const RawResourceAndroidNotificationSound('needs_you'),
      styleInformation: BigTextStyleInformation(body),
      category: AndroidNotificationCategory.reminder,
    );
    await _plugin.show(
      id: _inboxId(alert.item.id),
      title: alert.item.title,
      body: body,
      notificationDetails: NotificationDetails(android: android, iOS: DarwinNotificationDetails(threadIdentifier: 'inbox')),
      payload: '$inboxPayloadPrefix${alert.item.id}',
    );
  }

  @override
  Future<void> cancelInboxAlert(String itemId) async {
    if (!_ready) return;
    await _plugin.cancel(id: _inboxId(itemId));
  }

  Future<String?> launchSession() async {
    if (!_ready) return null;
    final NotificationAppLaunchDetails? details = await _plugin.getNotificationAppLaunchDetails();
    return details?.didNotificationLaunchApp == true ? details?.notificationResponse?.payload : null;
  }
}

const String inboxPayloadPrefix = 'inbox:';

const int inboxTabIndex = 3;

String alertPayload(AlertEvent event) => event.host.isEmpty ? storageKey('', event.sessionId) : event.session.qualified;

final Provider<void> notificationTapsProvider = Provider<void>((Ref ref) {
  final StreamSubscription<String> taps = ref.read(notificationServiceProvider).taps.listen((String payload) {
    if (payload.startsWith(inboxPayloadPrefix)) {
      ref.read(activeTabProvider.notifier).show(inboxTabIndex);
      return;
    }
    final String inUse = ref.read(macsProvider).active?.id ?? '';
    ref.read(selectedSessionProvider.notifier).select(SessionRef.fromPayload(payload, fallbackHost: inUse));
    ref.read(activeTabProvider.notifier).show(1);
  });
  ref.onDispose(taps.cancel);
});

final Provider<NotificationService> notificationServiceProvider = Provider<NotificationService>((Ref ref) => NotificationService());

final NotifierProvider<LifecycleNotifier, AppLifecycleState> lifecycleProvider = NotifierProvider<LifecycleNotifier, AppLifecycleState>(LifecycleNotifier.new);

class LifecycleNotifier extends Notifier<AppLifecycleState> {
  @override
  AppLifecycleState build() => AppLifecycleState.resumed;

  void set(AppLifecycleState next) => state = next;
}

Set<String> newlyNeedingInput(List<Session> before, List<Session> after) {
  final Set<String> already = <String>{for (final Session s in before) if (s.state == SessionState.needsInput) s.id};
  return <String>{for (final Session s in after) if (s.state == SessionState.needsInput && !already.contains(s.id)) s.id};
}

bool shouldNotify({required bool hadPreviousList, required AppLifecycleState lifecycle, required bool connected}) =>
    hadPreviousList && connected && lifecycle != AppLifecycleState.resumed;

final Provider<void> attentionWatcherProvider = Provider<void>((Ref ref) {
  final Map<String, AlertKind> announced = <String, AlertKind>{};
  final MessageAlertTracker tracker = MessageAlertTracker();
  ref.listen<SessionsState>(sessionsProvider, (SessionsState? previous, SessionsState next) {
    if (next.phase != LoadPhase.ready) return;
    final bool hadList = previous != null && previous.phase == LoadPhase.ready;
    final Set<String> muted = ref.read(alertPrefsProvider).mutedIn(ref.read(macsProvider).activeScope);
    final Map<String, AlertKind> stateAlerts = hadList ? alertsBetween(previous.sessions, next.sessions, announced) : <String, AlertKind>{};
    final List<MessageAlert> replies = tracker.update(next.sessions, now: DateTime.now(), muted: muted);
    if (!hadList) return;
    final Set<String> replied = <String>{for (final MessageAlert m in replies) m.sessionId};
    final Map<String, AlertKind> states = <String, AlertKind>{
      for (final MapEntry<String, AlertKind> e in stateAlerts.entries)
        if (!muted.contains(e.key) && !(e.value == AlertKind.finished && replied.contains(e.key))) e.key: e.value,
    };
    final List<AlertEvent> events = buildAlertEvents(
      host: ref.read(macsProvider).active?.id ?? '',
      sessions: next.sessions,
      stateAlerts: states,
      messages: replies,
      needsBody: L10nHolder.current.notificationsNeedsBody,
      finishedBody: L10nHolder.current.notificationsFinishedBody,
    );
    if (events.isEmpty) return;
    final AppLifecycleState lifecycle = ref.read(lifecycleProvider);
    if (lifecycle == AppLifecycleState.resumed) {
      final String? open = ref.read(activeTabProvider) == 1 ? ref.read(selectedSessionIdProvider) : null;
      final bool sound = ref.read(appSettingsProvider).sound;
      for (final AlertEvent event in events) {
        if (event.sessionId == open) continue;
        ref.read(inAppAlertProvider.notifier).show(event);
        if (!event.quiet) {
          if (sound) unawaited(ref.read(alertSoundsProvider).play(event.kind));
          unawaited(HapticFeedback.lightImpact());
        }
      }
      return;
    }
    final bool connected = ref.read(connectionStatusProvider).phase == LinkPhase.connected;
    if (!shouldNotify(hadPreviousList: true, lifecycle: lifecycle, connected: connected)) return;
    final NotificationService service = ref.read(notificationServiceProvider);
    for (final AlertEvent event in events) {
      unawaited(service.showAlert(event));
    }
  });
});

final Provider<void> inboxAlertWatcherProvider = Provider<void>((Ref ref) {
  final InboxAlerter alerter = InboxAlerter(
    sink: ref.read(notificationServiceProvider),
    nowMs: () => DateTime.now().millisecondsSinceEpoch,
  );
  String? owner;
  void sync() {
    final InboxState state = ref.read(inboxProvider);
    final String? active = ref.read(macsProvider).active?.id;
    if (owner != null && (owner != active || state.phase == InboxPhase.unsupported)) {
      owner = null;
      unawaited(alerter.sync(const <InboxItem>[]));
    }
    if (active == null || state.phase != InboxPhase.ready || state.macId != active) return;
    if (ref.read(connectionStatusProvider).phase != LinkPhase.connected) return;
    owner = active;
    unawaited(alerter.sync(state.items));
  }

  ref.listen<MacsState>(macsProvider, (MacsState? previous, MacsState next) => sync());
  ref.listen<InboxState>(inboxProvider, (InboxState? previous, InboxState next) => sync());
  ref.listen<ConnectionStatus>(connectionStatusProvider, (ConnectionStatus? previous, ConnectionStatus next) => sync());
  final Timer timer = Timer.periodic(const Duration(seconds: 5), (Timer _) => sync());
  ref.onDispose(timer.cancel);
});
