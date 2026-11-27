import 'dart:async';

import 'package:agent_console/app/notifications.dart';
import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/inbox/inbox_alerts.dart';
import 'package:flutter_local_notifications/flutter_local_notifications.dart';
import 'package:flutter_test/flutter_test.dart';

class _Shown {
  _Shown(this.id, this.title, this.body, this.channel);
  final int id;
  final String? title;
  final String? body;
  final String? channel;
}

class _FakePlugin implements FlutterLocalNotificationsPlugin {
  final List<_Shown> shown = <_Shown>[];
  final List<int> cancelled = <int>[];

  @override
  Future<bool?> initialize({required InitializationSettings settings, DidReceiveNotificationResponseCallback? onDidReceiveNotificationResponse, DidReceiveBackgroundNotificationResponseCallback? onDidReceiveBackgroundNotificationResponse}) async => true;

  @override
  Future<NotificationAppLaunchDetails?> getNotificationAppLaunchDetails() async => null;

  @override
  Future<void> show({required int id, String? title, String? body, NotificationDetails? notificationDetails, String? payload}) async {
    shown.add(_Shown(id, title, body, notificationDetails?.android?.channelId));
  }

  @override
  Future<void> cancel({required int id, String? tag}) async => cancelled.add(id);

  @override
  dynamic noSuchMethod(Invocation invocation) => Future<void>.value();
}

const int _start = 1793541465000;

Map<String, Object?> raw({
  String id = 'consent-1',
  String state = 'open',
  bool? blocking = true,
  int? waitingSince = _start,
  int? expiresMs,
}) =>
    <String, Object?>{
      'id': id,
      'kind': 'consent',
      'title': 'Claude Code asks to change a setting',
      'detail': 'attentionLevel',
      'createdMs': _start,
      'actions': <Object?>[],
      'state': state,
      'seen': false,
      'blocking': ?blocking,
      'waitingSince': ?waitingSince,
      'expiresMs': ?expiresMs,
    };

InboxItem item({String id = 'consent-1', String state = 'open', bool? blocking = true, int? waitingSince = _start, int? expiresMs}) =>
    InboxItem.fromJson(raw(id: id, state: state, blocking: blocking, waitingSince: waitingSince, expiresMs: expiresMs));

void main() {
  late _FakePlugin plugin;
  late int now;
  late InboxAlerter alerter;

  setUp(() async {
    plugin = _FakePlugin();
    now = _start;
    final NotificationService service = NotificationService(plugin);
    await service.init();
    alerter = InboxAlerter(sink: service, nowMs: () => now);
  });

  test('an item turning blocking posts one notification on needs_you_v3, and not again', () async {
    await alerter.sync(<InboxItem>[item(blocking: false)]);
    expect(plugin.shown, isEmpty);
    await alerter.sync(<InboxItem>[item()]);
    expect(plugin.shown, hasLength(1));
    expect(plugin.shown.single.channel, 'needs_you_v3');
    expect(plugin.shown.single.title, 'Claude Code asks to change a setting');
    await alerter.sync(<InboxItem>[item()]);
    now += 30000;
    await alerter.sync(<InboxItem>[item()]);
    expect(plugin.shown, hasLength(1));
  });

  test('an item without blocking posts none', () async {
    await alerter.sync(<InboxItem>[item(blocking: null, waitingSince: null)]);
    now += 400000;
    await alerter.sync(<InboxItem>[item(blocking: null, waitingSince: null)]);
    expect(plugin.shown, isEmpty);
  });

  test('the same item at 3 minutes posts the second, once', () async {
    await alerter.sync(<InboxItem>[item()]);
    now = _start + 179000;
    await alerter.sync(<InboxItem>[item()]);
    expect(plugin.shown, hasLength(1));
    now = _start + 180000;
    await alerter.sync(<InboxItem>[item()]);
    expect(plugin.shown, hasLength(2));
    expect(plugin.shown[1].id, plugin.shown[0].id);
    expect(plugin.shown[1].body, isNot(plugin.shown[0].body));
    now = _start + 200000;
    await alerter.sync(<InboxItem>[item()]);
    expect(plugin.shown, hasLength(2));
  });

  test('the last call comes a minute before the item expires, once', () async {
    final int expires = _start + 290000;
    await alerter.sync(<InboxItem>[item(expiresMs: expires)]);
    now = _start + 180000;
    await alerter.sync(<InboxItem>[item(expiresMs: expires)]);
    expect(plugin.shown, hasLength(2));
    now = expires - 60000;
    await alerter.sync(<InboxItem>[item(expiresMs: expires)]);
    expect(plugin.shown, hasLength(3));
    now = expires - 30000;
    await alerter.sync(<InboxItem>[item(expiresMs: expires)]);
    expect(plugin.shown, hasLength(3));
  });

  test('an item first seen late posts one notification, not one per rung', () async {
    now = _start + 250000;
    await alerter.sync(<InboxItem>[item(expiresMs: _start + 290000)]);
    expect(plugin.shown, hasLength(1));
    await alerter.sync(<InboxItem>[item(expiresMs: _start + 290000)]);
    expect(plugin.shown, hasLength(1));
  });

  test('closing an item cancels its notification once', () async {
    await alerter.sync(<InboxItem>[item()]);
    final int id = plugin.shown.single.id;
    await alerter.sync(<InboxItem>[item(state: 'answered')]);
    expect(plugin.cancelled, <int>[id]);
    await alerter.sync(<InboxItem>[item(state: 'answered')]);
    expect(plugin.cancelled, <int>[id]);
  });

  test('an item removed from the list and one that stops blocking are cancelled', () async {
    await alerter.sync(<InboxItem>[item(), item(id: 'consent-2')]);
    expect(plugin.shown, hasLength(2));
    await alerter.sync(<InboxItem>[item(blocking: false)]);
    expect(plugin.cancelled, hasLength(2));
  });

  test('a closed item never posts a later rung', () async {
    await alerter.sync(<InboxItem>[item()]);
    now = _start + 200000;
    await alerter.sync(<InboxItem>[item(state: 'expired')]);
    expect(plugin.shown, hasLength(1));
  });

  test('an item without waitingSince counts from when it was first seen', () async {
    await alerter.sync(<InboxItem>[item(waitingSince: null)]);
    now += 180000;
    await alerter.sync(<InboxItem>[item(waitingSince: null)]);
    expect(plugin.shown, hasLength(2));
  });

  test('a cancel pass that starts while a show is pending still cancels every item', () async {
    final _SlowSink sink = _SlowSink();
    final InboxAlerter slow = InboxAlerter(sink: sink, nowMs: () => now);
    final Future<void> first = slow.sync(<InboxItem>[item(), item(id: 'consent-2')]);
    final Future<void> cancel = slow.sync(const <InboxItem>[]);
    await Future<void>.delayed(Duration.zero);
    sink.release();
    await Future.wait(<Future<void>>[first, cancel]);
    final Set<String> shown = <String>{for (final String e in sink.events) if (e.startsWith('show:')) e.substring(5)};
    final Set<String> cancelled = <String>{for (final String e in sink.events) if (e.startsWith('cancel:')) e.substring(7)};
    expect(shown, <String>{'consent-1', 'consent-2'});
    expect(cancelled, shown);
    expect(sink.events.indexOf('cancel:consent-2'), greaterThan(sink.events.indexOf('show:consent-2')));
  });
}

class _SlowSink implements InboxAlertSink {
  final List<String> events = <String>[];
  final Completer<void> _gate = Completer<void>();

  void release() => _gate.complete();

  @override
  Future<void> showInboxAlert(InboxAlert alert) async {
    await _gate.future;
    events.add('show:${alert.item.id}');
  }

  @override
  Future<void> cancelInboxAlert(String itemId) async => events.add('cancel:$itemId');
}
