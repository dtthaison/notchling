import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/sessions/node_label.dart';
import 'package:agent_console/features/sessions/session_row.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/harness.dart';

final DateTime _now = DateTime(2026, 11, 2, 12);

Map<String, Object?> _wire({Object? node = _absent}) => <String, Object?>{
  'id': '0123456789abcdef/job-7',
  'name': 'Build the parser',
  'project': 'parser',
  'cwd': '/work/parser',
  'entrypoint': 'job',
  'state': 'running',
  'control': 'job',
  'lastActivity': _now
      .subtract(const Duration(minutes: 3))
      .millisecondsSinceEpoch
      .toDouble(),
  if (!identical(node, _absent)) 'node': node,
};

const Object _absent = Object();

Widget _row(Session session) =>
    host(SessionRow(session: session, now: _now, onTap: () {}));

void main() {
  testWidgets('shows_node_label_for_remote_session', (
    WidgetTester tester,
  ) async {
    final Session session = Session.fromJson(
      _wire(
        node: <String, Object?>{
          'id': '0123456789abcdef',
          'name': 'mac-mini',
          'online': true,
        },
      ),
    );
    expect(session.node?.name, 'mac-mini');
    await tester.pumpWidget(_row(session));
    expect(find.byType(NodeLabel), findsOneWidget);
    expect(find.text('mac-mini'), findsOneWidget);
    expect(find.text('Offline'), findsNothing);
    expect(find.bySemanticsLabel(RegExp('On mac-mini')), findsOneWidget);
  });

  testWidgets('no_label_without_node', (WidgetTester tester) async {
    for (final Object? node in <Object?>[
      _absent,
      null,
      'mac-mini',
      3,
      <Object?>[],
      <String, Object?>{},
    ]) {
      final Session session = Session.fromJson(_wire(node: node));
      expect(session.node, isNull, reason: '$node');
      await tester.pumpWidget(_row(session));
      expect(find.byType(NodeLabel), findsNothing, reason: '$node');
      expect(find.text('Build the parser'), findsOneWidget);
    }
  });

  testWidgets('offline_node_label_is_dimmed', (WidgetTester tester) async {
    final Session session = Session.fromJson(
      _wire(
        node: <String, Object?>{
          'id': '0123456789abcdef',
          'name': 'mac-mini',
          'online': false,
        },
      ),
    );
    await tester.pumpWidget(_row(session));
    expect(find.text('mac-mini'), findsOneWidget);
    expect(find.text('Offline'), findsOneWidget);
    final Opacity dimmed = tester.widget<Opacity>(
      find.descendant(
        of: find.byType(NodeLabel),
        matching: find.byType(Opacity),
      ),
    );
    expect(dimmed.opacity, lessThan(1));
    expect(
      find.bySemanticsLabel(RegExp('On mac-mini, offline')),
      findsOneWidget,
    );

    final Session online = Session.fromJson(
      _wire(
        node: <String, Object?>{
          'id': '0123456789abcdef',
          'name': 'mac-mini',
          'online': true,
        },
      ),
    );
    await tester.pumpWidget(_row(online));
    expect(
      find.descendant(
        of: find.byType(NodeLabel),
        matching: find.byType(Opacity),
      ),
      findsNothing,
    );
  });

  testWidgets('a_node_with_a_long_name_and_big_text_does_not_overflow', (
    WidgetTester tester,
  ) async {
    final Session session = Session.fromJson(
      _wire(
        node: <String, Object?>{
          'id': '0123456789abcdef',
          'name': 'a-very-long-computer-name-in-the-cluster',
          'online': false,
        },
      ),
    );
    await tester.pumpWidget(
      host(SessionRow(session: session, now: _now, onTap: () {}), textScale: 3),
    );
    expect(tester.takeException(), isNull);
  });
}
