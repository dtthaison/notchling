import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/chat/takeover_flow.dart';
import 'package:agent_console/features/sessions/session_row.dart';
import 'package:agent_console/features/sessions/takeover_row_button.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/fake_ws.dart';
import 'support/harness.dart';

Session _view({Map<String, Object?>? takeover, String state = 'done'}) =>
    Session.fromJson(sessionJson('a', state: state, control: 'viewOnly', takeover: takeover));

void main() {
  group('who gets a button', () {
    test('only view-only sessions the Mac says it can take over, now or after stopping', () {
      expect(canTakeOver(_view(takeover: <String, Object?>{'available': true})), isTrue);
      expect(canTakeOver(_view(takeover: <String, Object?>{'available': false, 'stoppable': true})), isTrue);
      expect(canTakeOver(_view(takeover: <String, Object?>{'available': false, 'reason': 'Không có transcript'})), isFalse);
      expect(canTakeOver(_view()), isFalse, reason: 'an older Mac says nothing');
      expect(canTakeOver(Session.fromJson(sessionJson('c', takeover: <String, Object?>{'available': true}))), isFalse, reason: 'already controllable');
    });
  });

  group('confirmation', () {
    Future<TakeoverChoice?> run(WidgetTester tester, Session session, List<String> taps) async {
      TakeoverChoice? result;
      await tester.pumpWidget(host(Builder(
        builder: (BuildContext context) => TextButton(onPressed: () async => result = await confirmTakeover(context, session), child: const Text('go')),
      )));
      await tester.tap(find.text('go'));
      await tester.pumpAndSettle();
      for (final String label in taps) {
        await tester.tap(find.text(label));
        await tester.pumpAndSettle();
      }
      return result;
    }

    testWidgets('an exited session: one confirmation to resume', (WidgetTester tester) async {
      final TakeoverChoice? result = await run(tester, _view(takeover: <String, Object?>{'available': true}), <String>['Take over']);
      expect(result, TakeoverChoice.resume);
    });

    testWidgets('a running original that is idle: one confirmation that says it will be stopped', (WidgetTester tester) async {
      final Session session = _view(takeover: <String, Object?>{'available': false, 'stoppable': true, 'busy': false});
      TakeoverChoice? result;
      await tester.pumpWidget(host(Builder(
        builder: (BuildContext context) => TextButton(onPressed: () async => result = await confirmTakeover(context, session), child: const Text('go')),
      )));
      await tester.tap(find.text('go'));
      await tester.pumpAndSettle();
      expect(find.textContaining('Stop the session on the Mac (Claude desktop / VS Code) and reopen it in Console'), findsOneWidget);
      expect(find.textContaining('the current turn will be cut off'), findsNothing);
      await tester.tap(find.text('Stop and take over'));
      await tester.pumpAndSettle();
      expect(result, TakeoverChoice.stopAndResume);
    });

    testWidgets('a busy original needs a second, explicit confirmation', (WidgetTester tester) async {
      final Session session = _view(state: 'running', takeover: <String, Object?>{'available': false, 'stoppable': true, 'busy': true});
      TakeoverChoice? result;
      await tester.pumpWidget(host(Builder(
        builder: (BuildContext context) => TextButton(onPressed: () async => result = await confirmTakeover(context, session), child: const Text('go')),
      )));
      await tester.tap(find.text('go'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Stop and take over'));
      await tester.pumpAndSettle();
      expect(result, isNull, reason: 'not decided yet: the warning is showing');
      expect(find.text('The session is running; the current turn will be cut off.'), findsOneWidget);
      await tester.tap(find.text('Stop anyway'));
      await tester.pumpAndSettle();
      expect(result, TakeoverChoice.stopAndResume);
    });

    testWidgets('cancelling at the second confirmation stops nothing', (WidgetTester tester) async {
      final Session session = _view(state: 'running', takeover: <String, Object?>{'available': false, 'stoppable': true, 'busy': true});
      final TakeoverChoice? result = await run(tester, session, <String>['Stop and take over', 'Cancel']);
      expect(result, TakeoverChoice.cancel);
    });

    testWidgets('cancelling the first one stops nothing either', (WidgetTester tester) async {
      final TakeoverChoice? result = await run(tester, _view(takeover: <String, Object?>{'available': false, 'stoppable': true}), <String>['Cancel']);
      expect(result, TakeoverChoice.cancel);
    });
  });

  group('row button', () {
    testWidgets('sits at the end of the row with a target of at least 48 dp', (WidgetTester tester) async {
      final Session session = _view(takeover: <String, Object?>{'available': true});
      await tester.pumpWidget(ProviderScope(child: host(SessionRow(session: session, now: DateTime(2026, 11, 3), onTap: () {}, trailing: TakeoverRowButton(session: session)))));
      expect(find.text('Take over'), findsOneWidget);
      final Size size = tester.getSize(find.ancestor(of: find.text('Take over'), matching: find.byType(ConstrainedBox)).first);
      expect(size.height, greaterThanOrEqualTo(48));
    });
  });
}
