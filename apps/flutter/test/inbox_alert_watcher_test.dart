import 'package:agent_console/app/notifications.dart';
import 'package:agent_console/core/net/bridge_client.dart';
import 'package:agent_console/core/pairing/mac_profile.dart';
import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/connection/connection.dart';
import 'package:agent_console/features/inbox/inbox_notifier.dart';
import 'package:agent_console/features/macs/macs_notifier.dart';
import 'package:flutter_local_notifications/flutter_local_notifications.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/two_computers.dart';

class _FakePlugin implements FlutterLocalNotificationsPlugin {
  final List<int> shown = <int>[];
  final List<int> cancelled = <int>[];

  @override
  Future<bool?> initialize({required InitializationSettings settings, DidReceiveNotificationResponseCallback? onDidReceiveNotificationResponse, DidReceiveBackgroundNotificationResponseCallback? onDidReceiveBackgroundNotificationResponse}) async => true;

  @override
  Future<NotificationAppLaunchDetails?> getNotificationAppLaunchDetails() async => null;

  @override
  Future<void> show({required int id, String? title, String? body, NotificationDetails? notificationDetails, String? payload}) async => shown.add(id);

  @override
  Future<void> cancel({required int id, String? tag}) async => cancelled.add(id);

  @override
  dynamic noSuchMethod(Invocation invocation) => Future<void>.value();
}

class _Macs extends MacsNotifier {
  @override
  MacsState build() => const MacsState(macs: <MacProfile>[macA, macB], activeId: 'aaaaaaaaaaaaaaaa', loaded: true);
  void use(String? id) => state = MacsState(macs: state.macs, activeId: id, loaded: true);
  void drop() => state = const MacsState(macs: <MacProfile>[macB], loaded: true);
}

class _Inbox extends InboxNotifier {
  @override
  InboxState build() => InboxState(
        phase: InboxPhase.ready,
        macId: 'aaaaaaaaaaaaaaaa',
        items: <InboxItem>[
          InboxItem.fromJson(<String, Object?>{
            'id': 'c1',
            'kind': 'consent',
            'title': 'T',
            'detail': '',
            'createdMs': 1,
            'actions': <Object?>[],
            'state': 'open',
            'seen': false,
            'blocking': true,
            'waitingSince': DateTime.now().millisecondsSinceEpoch,
          }),
        ],
      );
  void phase(InboxPhase phase) => state = InboxState(phase: phase, items: state.items, macId: state.macId);
  void reload(String macId) => state = InboxState(phase: InboxPhase.ready, items: state.items, macId: macId);
  void empty(InboxPhase phase) => state = InboxState(phase: phase);
}

class _Link extends ConnectionStatusNotifier {
  @override
  ConnectionStatus build() => const ConnectionStatus(LinkPhase.connected);
  void set(LinkPhase phase) => state = ConnectionStatus(phase);
}

void main() {
  late _FakePlugin plugin;
  late ProviderContainer container;

  setUp(() async {
    plugin = _FakePlugin();
    final NotificationService service = NotificationService(plugin);
    await service.init();
    container = ProviderContainer(overrides: [
      notificationServiceProvider.overrideWithValue(service),
      macsProvider.overrideWith(_Macs.new),
      inboxProvider.overrideWith(_Inbox.new),
      connectionStatusProvider.overrideWith(_Link.new),
    ]);
    addTearDown(container.dispose);
    container.listen<void>(inboxAlertWatcherProvider, (void a, void b) {});
    (container.read(inboxProvider.notifier) as _Inbox).phase(InboxPhase.ready);
    await Future<void>.delayed(Duration.zero);
  });

  test('the alert is posted for the active Mac', () {
    expect(plugin.shown, hasLength(1));
    expect(plugin.cancelled, isEmpty);
  });

  test('a short disconnect from the same Mac keeps the alert', () async {
    (container.read(connectionStatusProvider.notifier) as _Link).set(LinkPhase.disconnected);
    (container.read(inboxProvider.notifier) as _Inbox).phase(InboxPhase.loading);
    await Future<void>.delayed(Duration.zero);
    (container.read(connectionStatusProvider.notifier) as _Link).set(LinkPhase.connected);
    (container.read(inboxProvider.notifier) as _Inbox).phase(InboxPhase.ready);
    await Future<void>.delayed(Duration.zero);
    expect(plugin.cancelled, isEmpty);
    expect(plugin.shown, hasLength(1));
  });

  test('removing the active Mac cancels the alert', () async {
    (container.read(macsProvider.notifier) as _Macs).drop();
    (container.read(inboxProvider.notifier) as _Inbox).empty(InboxPhase.loading);
    await Future<void>.delayed(Duration.zero);
    expect(plugin.cancelled, hasLength(1));
  });

  test('switching to another Mac cancels the alert and does not post the old list again', () async {
    (container.read(macsProvider.notifier) as _Macs).use('bbbbbbbbbbbbbbbb');
    await Future<void>.delayed(Duration.zero);
    expect(plugin.cancelled, hasLength(1));
    expect(plugin.shown, hasLength(1));
    (container.read(inboxProvider.notifier) as _Inbox).empty(InboxPhase.loading);
    await Future<void>.delayed(Duration.zero);
    expect(plugin.shown, hasLength(1));
  });

  test('the new Mac\'s own items post once its inbox has loaded', () async {
    (container.read(macsProvider.notifier) as _Macs).use('bbbbbbbbbbbbbbbb');
    await Future<void>.delayed(Duration.zero);
    (container.read(inboxProvider.notifier) as _Inbox).reload('bbbbbbbbbbbbbbbb');
    await Future<void>.delayed(Duration.zero);
    expect(plugin.shown, hasLength(2));
  });

  test('an unsupported inbox cancels the alert', () async {
    (container.read(inboxProvider.notifier) as _Inbox).empty(InboxPhase.unsupported);
    await Future<void>.delayed(Duration.zero);
    expect(plugin.cancelled, hasLength(1));
  });
}
