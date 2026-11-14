import 'package:agent_console/core/net/bridge_client.dart';
import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/macs/computer_heading.dart';
import 'package:agent_console/features/sessions/sessions_notifier.dart';
import 'package:agent_console/features/sessions/sessions_screen.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/harness.dart';

Session _session(String id, String name, SessionState state) => Session(
      id: id,
      name: name,
      project: 'p',
      cwd: '/p',
      entrypoint: 'cli',
      state: state,
      control: SessionControl.console,
      lastActivity: DateTime(2026, 11, 8, 12),
    );

void main() {
  group('sessions grouped per computer', () {
    final List<Session> sessions = <Session>[
      _session('aaaaaaaaaaaaaaaa/s1', 'Session on A', SessionState.running),
      _session('bbbbbbbbbbbbbbbb/s1', 'Session on B', SessionState.needsInput),
    ];
    const List<ComputerHeading> two = <ComputerHeading>[
      ComputerHeading(id: 'aaaaaaaaaaaaaaaa', name: 'Mac A', status: ConnectionStatus(LinkPhase.connected)),
      ComputerHeading(id: 'bbbbbbbbbbbbbbbb', name: 'Linux B', status: ConnectionStatus(LinkPhase.disconnected)),
    ];

    Widget view(List<ComputerHeading> computers, {void Function(Session)? onSelect, String? selectedId, List<Session>? list}) => host(
          SizedBox(
            height: 1600,
            child: SessionsView(
              state: SessionsState(phase: LoadPhase.ready, sessions: list ?? sessions),
              computers: computers,
              expanded: const <String>{},
              selectedId: selectedId,
              query: '',
              project: null,
              onQuery: (String _) {},
              onProject: (String? _) {},
              onSelect: onSelect ?? (Session _) {},
              onToggleExpand: (String _) {},
              onRefresh: () async {},
              onNewSession: () {},
              onArchive: (Session _) {},
              onTakeover: (Session _) {},
              now: DateTime(2026, 11, 8, 12, 5),
            ),
          ),
        );

    testWidgets('each computer has a heading with its state, and its own sessions under it', (WidgetTester tester) async {
      await tester.pumpWidget(view(two));
      await tester.pumpAndSettle();
      expect(find.text('Mac A'), findsOneWidget);
      expect(find.text('Linux B'), findsOneWidget);
      expect(find.text('Connected'), findsOneWidget);
      expect(find.text('Not connected'), findsOneWidget);
      final double a = tester.getTopLeft(find.text('Mac A')).dy;
      final double onA = tester.getTopLeft(find.text('Session on A')).dy;
      final double b = tester.getTopLeft(find.text('Linux B')).dy;
      final double onB = tester.getTopLeft(find.text('Session on B')).dy;
      expect(a < onA && onA < b && b < onB, isTrue, reason: 'A, its session, then B and its session');
    });

    testWidgets('tapping s1 under B picks B’s s1, never A’s', (WidgetTester tester) async {
      Session? picked;
      await tester.pumpWidget(view(two, onSelect: (Session s) => picked = s));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Session on B'));
      expect(picked?.id, 'bbbbbbbbbbbbbbbb/s1');
    });

    testWidgets('only the picked computer’s s1 shows as selected', (WidgetTester tester) async {
      await tester.pumpWidget(view(two, selectedId: 'bbbbbbbbbbbbbbbb/s1'));
      await tester.pumpAndSettle();
      final Iterable<Semantics> selected = tester
          .widgetList<Semantics>(find.byType(Semantics))
          .where((Semantics s) => s.properties.selected == true && (s.properties.label ?? '').contains('Session on'));
      expect(selected, isNotEmpty);
      expect(selected.map((Semantics s) => s.properties.label), everyElement(contains('Session on B')));
    });

    testWidgets('a computer with nothing to show says so under its heading', (WidgetTester tester) async {
      await tester.pumpWidget(view(two, list: <Session>[sessions.first]));
      await tester.pumpAndSettle();
      expect(find.text('Linux B'), findsOneWidget);
      expect(find.text('No sessions'), findsOneWidget);
    });

    testWidgets('with one computer there is no heading: the list looks as it always did', (WidgetTester tester) async {
      await tester.pumpWidget(view(two.take(1).toList(), list: <Session>[sessions.first]));
      await tester.pumpAndSettle();
      expect(find.text('Mac A'), findsNothing);
      expect(find.text('Session on A'), findsOneWidget);
    });
  });
}
