import 'package:agent_console/app/alerts.dart';
import 'package:agent_console/app/message_alerts.dart';
import 'package:agent_console/app/notifications.dart';
import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/macs/session_ref.dart';
import 'package:agent_console/features/sessions/sessions_notifier.dart';
import 'package:agent_console/features/sessions/takeover_row_button.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/fake_ws.dart';
import 'support/harness.dart';

const String remote = '0123456789abcdef/s1';

void main() {
  group('a remote session id is not a computer’s', () {
    test('it is read as a computer id only when several computers are paired', () {
      expect(SessionRef.onComputer(remote, manyComputers: true), const SessionRef('0123456789abcdef', 's1'));
      expect(SessionRef.onComputer(remote, manyComputers: false), isNull);
      expect(SessionRef.onComputer('s1', manyComputers: true), isNull);
    });

    test('a notification for it with no computer in use survives the tap', () {
      const AlertEvent event = AlertEvent(sessionId: remote, name: 'n', body: 'b', kind: AlertKind.message);
      final String payload = alertPayload(event);
      expect(payload, '/$remote');
      expect(SessionRef.fromPayload(payload, fallbackHost: 'aaaaaaaaaaaaaaaa'), const SessionRef('aaaaaaaaaaaaaaaa', remote));
    });

    test('with a computer in use the payload still names that computer first', () {
      const AlertEvent event = AlertEvent(host: 'aaaaaaaaaaaaaaaa', sessionId: remote, name: 'n', body: 'b', kind: AlertKind.message);
      expect(alertPayload(event), 'aaaaaaaaaaaaaaaa/$remote');
      expect(SessionRef.fromPayload(alertPayload(event), fallbackHost: 'bbbbbbbbbbbbbbbb'), const SessionRef('aaaaaaaaaaaaaaaa', remote));
    });

    test('an old payload with no host still opens on the computer in use', () {
      expect(SessionRef.fromPayload('s1', fallbackHost: 'aaaaaaaaaaaaaaaa'), const SessionRef('aaaaaaaaaaaaaaaa', 's1'));
    });

    testWidgets('Take over on a remote session of the one paired Mac asks that Mac with the whole id', (WidgetTester tester) async {
      final Map<String, Object?> row = sessionJson(remote, state: 'done', control: 'viewOnly', takeover: <String, Object?>{'available': true})
        ..['node'] = <String, Object?>{'id': '0123456789abcdef', 'name': 'build-box', 'online': true};
      final FakeWs socket = FakeWs(onSend: (FakeWs ws, Map<String, Object?> message) {
        switch (message['op']) {
          case 'hello':
            ws.reply(message['id']!);
          case 'sessions.list':
            ws.reply(message['id']!, data: <String, Object?>{'sessions': <Object?>[row]});
          case 'subscribe':
            ws.reply(message['id']!, data: <String, Object?>{'subscribed': true});
          case 'session.takeover':
            ws.reply(message['id']!, data: <String, Object?>{'sessionId': 'resumed-1', 'resumed': true});
        }
      });
      final ProviderContainer container = containerWith(socket);
      await tester.runAsync(() => connect(container));
      final Session session = Session.fromJson(row);
      await tester.pumpWidget(UncontrolledProviderScope(
        container: container,
        child: host(Row(children: <Widget>[TakeoverRowButton(session: session)])),
      ));
      await tester.tap(find.text('Take over'));
      await tester.pump(const Duration(milliseconds: 400));
      await tester.tap(find.descendant(of: find.byType(AlertDialog), matching: find.text('Take over')));
      await tester.pump();
      await tester.runAsync(() => until(() => socket.sent.any((Map<String, Object?> m) => m['op'] == 'session.takeover')));
      final Map<String, Object?> request = socket.sent.firstWhere((Map<String, Object?> m) => m['op'] == 'session.takeover');
      expect(request['sessionId'], remote, reason: 'not "s1" on a computer called 0123456789abcdef');
      await tester.runAsync(() => until(() => container.read(selectedSessionIdProvider) == 'resumed-1'));
      await tester.pumpWidget(const SizedBox.shrink());
    });
  });
}
