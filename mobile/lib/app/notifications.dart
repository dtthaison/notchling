import 'dart:async';

import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_local_notifications/flutter_local_notifications.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../core/net/bridge_client.dart';
import '../core/protocol/models.dart';
import '../features/connection/connection.dart';
import '../features/sessions/sessions_notifier.dart';
import 'alerts.dart';
import 'message_alerts.dart';
import 'nav_state.dart';
import '../features/settings/app_settings.dart';

/// Local notifications only: the phone tells you a session needs input while the app is in the background
/// but still connected. Waking a closed app needs APNs (iOS) or FCM (Android) and a server to send them;
/// that is a later phase and not built here.
class NotificationService {
  NotificationService([FlutterLocalNotificationsPlugin? plugin]) : _plugin = plugin ?? FlutterLocalNotificationsPlugin();
  final FlutterLocalNotificationsPlugin _plugin;
  final StreamController<String> _taps = StreamController<String>.broadcast();
  bool _ready = false;

  /// Session ids of tapped notifications.
  Stream<String> get taps => _taps.stream;

  Future<void> init() async {
    if (_ready) return;
    await _plugin.initialize(
      settings: const InitializationSettings(
        android: AndroidInitializationSettings('@mipmap/ic_launcher'),
        iOS: DarwinInitializationSettings(requestAlertPermission: false, requestBadgePermission: false, requestSoundPermission: false),
      ),
      onDidReceiveNotificationResponse: (NotificationResponse response) {
        final String? payload = response.payload;
        if (payload != null) _taps.add(payload);
      },
    );
    _ready = true;
    // The app was started by tapping a notification: open that session once the first screen is up.
    final String? launched = await launchSession();
    if (launched != null) Future<void>.delayed(const Duration(milliseconds: 800), () => _taps.add(launched));
  }

  /// Asks once the first time a Mac is connected, not at launch, so the system prompt comes with a reason.
  Future<void> requestPermission() async {
    if (!_ready) return;
    await _plugin.resolvePlatformSpecificImplementation<IOSFlutterLocalNotificationsPlugin>()?.requestPermissions(alert: true);
    await _plugin.resolvePlatformSpecificImplementation<AndroidFlutterLocalNotificationsPlugin>()?.requestNotificationsPermission();
  }

  /// One system notification per session (the session id is its tag, so a later alert replaces the earlier one),
  /// grouped under a summary. Needs-you uses a heads-up channel with its own chime, replies a softer default channel.
  /// Android fixes a channel's sound when the channel is created, so the channels carry a version in their ids.
  Future<void> showAlert(AlertEvent event) async {
    if (!_ready) return;
    final bool needs = event.kind == AlertKind.needsInput;
    final AndroidNotificationDetails android = AndroidNotificationDetails(
      needs ? 'needs_you_v2' : (event.kind == AlertKind.finished ? 'finished_v2' : 'messages_v2'),
      needs ? 'Session cần bạn' : (event.kind == AlertKind.finished ? 'Session xong' : 'Tin mới từ Claude'),
      importance: needs ? Importance.high : Importance.defaultImportance,
      priority: needs ? Priority.high : Priority.defaultPriority,
      sound: event.kind == AlertKind.finished ? null : RawResourceAndroidNotificationSound(needs ? 'needs_you' : 'new_message'),
      playSound: event.kind != AlertKind.finished,
      tag: event.sessionId,
      groupKey: _group,
      // A quiet update (a burst of replies) changes the text without sounding again.
      onlyAlertOnce: event.quiet,
      styleInformation: BigTextStyleInformation(event.body),
      category: AndroidNotificationCategory.message,
    );
    await _plugin.show(
      id: event.sessionId.hashCode & 0x7fffffff,
      title: event.name,
      body: event.body,
      notificationDetails: NotificationDetails(android: android, iOS: DarwinNotificationDetails(threadIdentifier: _group, presentSound: !event.quiet)),
      payload: event.sessionId,
    );
    await _plugin.show(
      id: 1,
      title: 'Agent Console',
      body: 'Có cập nhật mới',
      notificationDetails: const NotificationDetails(
        android: AndroidNotificationDetails('messages_v2', 'Tin mới từ Claude', groupKey: _group, setAsGroupSummary: true, onlyAlertOnce: true, playSound: false, importance: Importance.low, priority: Priority.low),
        iOS: DarwinNotificationDetails(threadIdentifier: _group),
      ),
    );
  }

  static const String _group = 'sessions';

  /// The session whose notification started the app, when it was launched by tapping one.
  Future<String?> launchSession() async {
    if (!_ready) return null;
    final NotificationAppLaunchDetails? details = await _plugin.getNotificationAppLaunchDetails();
    return details?.didNotificationLaunchApp == true ? details?.notificationResponse?.payload : null;
  }
}

final Provider<NotificationService> notificationServiceProvider = Provider<NotificationService>((Ref ref) => NotificationService());

/// The app lifecycle, so background-only behaviour (notifications) can tell where the app is.
final NotifierProvider<LifecycleNotifier, AppLifecycleState> lifecycleProvider = NotifierProvider<LifecycleNotifier, AppLifecycleState>(LifecycleNotifier.new);

class LifecycleNotifier extends Notifier<AppLifecycleState> {
  @override
  AppLifecycleState build() => AppLifecycleState.resumed;

  void set(AppLifecycleState next) => state = next;
}

/// Ids of sessions that are newly in "Cần input" between two lists.
Set<String> newlyNeedingInput(List<Session> before, List<Session> after) {
  final Set<String> already = <String>{for (final Session s in before) if (s.state == SessionState.needsInput) s.id};
  return <String>{for (final Session s in after) if (s.state == SessionState.needsInput && !already.contains(s.id)) s.id};
}

/// Whether a notification should fire: not on the first list we ever see, and only while backgrounded
/// and connected.
bool shouldNotify({required bool hadPreviousList, required AppLifecycleState lifecycle, required bool connected}) =>
    hadPreviousList && connected && lifecycle != AppLifecycleState.resumed;

/// Watches the session list and announces what is worth the user's attention:
/// - a session that now needs the user (permission, question);
/// - a new reply from Claude (the session's `lastMessage` id changed), for every session except the chat that is open;
/// - a turn that finished.
/// With the app open this is a banner at the top (tap opens that chat) with a soft sound and a light tap; in the
/// background it is a system notification per session. Muted sessions say nothing.
final Provider<void> attentionWatcherProvider = Provider<void>((Ref ref) {
  final Map<String, AlertKind> announced = <String, AlertKind>{};
  final MessageAlertTracker tracker = MessageAlertTracker();
  ref.listen<SessionsState>(sessionsProvider, (SessionsState? previous, SessionsState next) {
    if (next.phase != LoadPhase.ready) return;
    final bool hadList = previous != null && previous.phase == LoadPhase.ready;
    final Set<String> muted = ref.read(alertPrefsProvider).muted;
    // Track every change, even when nothing is shown, so a later alert does not repeat an old one.
    final Map<String, AlertKind> stateAlerts = hadList ? alertsBetween(previous.sessions, next.sessions, announced) : <String, AlertKind>{};
    final List<MessageAlert> replies = tracker.update(next.sessions, now: DateTime.now(), muted: muted);
    if (!hadList) return;
    // A reply covers "finished": it says more.
    final Set<String> replied = <String>{for (final MessageAlert m in replies) m.sessionId};
    final Map<String, AlertKind> states = <String, AlertKind>{
      for (final MapEntry<String, AlertKind> e in stateAlerts.entries)
        if (!muted.contains(e.key) && !(e.value == AlertKind.finished && replied.contains(e.key))) e.key: e.value,
    };
    final List<AlertEvent> events = buildAlertEvents(
      sessions: next.sessions,
      stateAlerts: states,
      messages: replies,
      needsBody: 'Cần bạn trả lời',
      finishedBody: 'Đã xong',
    );
    if (events.isEmpty) return;
    final AppLifecycleState lifecycle = ref.read(lifecycleProvider);
    if (lifecycle == AppLifecycleState.resumed) {
      // The chat the user is looking at needs no banner.
      // The chat is a page pushed over the list now, not a tab.
      final String? open = ref.read(chatRouteOpenProvider) ? ref.read(selectedSessionIdProvider) : null;
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
