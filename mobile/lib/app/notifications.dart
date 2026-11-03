import 'dart:async';

import 'package:flutter/widgets.dart';
import 'package:flutter_local_notifications/flutter_local_notifications.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../core/net/bridge_client.dart';
import '../core/protocol/models.dart';
import '../features/connection/connection.dart';
import '../features/sessions/sessions_notifier.dart';
import 'alerts.dart';

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

  Future<void> showNeedsInput({required String sessionId, required String title, required String body}) =>
      _show(sessionId, title, body, const AndroidNotificationDetails('needs_input', 'Cần input', importance: Importance.high, priority: Priority.high));

  Future<void> showFinished({required String sessionId, required String title, required String body}) =>
      _show(sessionId, title, body, const AndroidNotificationDetails('finished', 'Session xong', importance: Importance.defaultImportance, priority: Priority.defaultPriority));

  Future<void> _show(String sessionId, String title, String body, AndroidNotificationDetails android) async {
    if (!_ready) return;
    await _plugin.show(
      // One notification per session: a later alert for the same session replaces the earlier one.
      id: sessionId.hashCode & 0x7fffffff,
      title: title,
      body: body,
      notificationDetails: NotificationDetails(android: android, iOS: const DarwinNotificationDetails()),
      payload: sessionId,
    );
  }

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

final Provider<void> attentionWatcherProvider = Provider<void>((Ref ref) {
  final Map<String, AlertKind> announced = <String, AlertKind>{};
  ref.listen<SessionsState>(sessionsProvider, (SessionsState? previous, SessionsState next) {
    if (previous == null || previous.phase != LoadPhase.ready) return;
    // Track every change, even while in the foreground, so a later background poll does not repeat an old one.
    final Map<String, AlertKind> alerts = alertsBetween(previous.sessions, next.sessions, announced);
    final bool connected = ref.read(connectionStatusProvider).phase == LinkPhase.connected;
    if (!shouldNotify(hadPreviousList: true, lifecycle: ref.read(lifecycleProvider), connected: connected)) return;
    final Set<String> muted = ref.read(alertPrefsProvider).muted;
    for (final MapEntry<String, AlertKind> alert in alerts.entries) {
      final Session? session = next.byId(alert.key);
      if (session == null || muted.contains(session.id)) continue;
      final NotificationService service = ref.read(notificationServiceProvider);
      switch (alert.value) {
        case AlertKind.needsInput:
          unawaited(service.showNeedsInput(sessionId: session.id, title: 'Session cần input', body: session.name));
        case AlertKind.finished:
          unawaited(service.showFinished(sessionId: session.id, title: 'Session đã xong', body: session.name));
      }
    }
  });
});
