import 'package:agent_console/app/nav_state.dart';
import 'package:agent_console/app/shell.dart';
import 'package:agent_console/core/net/bridge_client.dart';
import 'package:agent_console/features/connection/connection.dart';
import 'package:agent_console/features/sessions/sessions_notifier.dart';
import 'package:agent_console/features/sessions/sessions_screen.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:flutter_test/flutter_test.dart';

import 'support/harness.dart';

class _Sessions extends SessionsNotifier {
  @override
  SessionsState build() => const SessionsState(phase: LoadPhase.ready);
}

class _Status extends ConnectionStatusNotifier {
  @override
  ConnectionStatus build() => const ConnectionStatus(LinkPhase.disconnected);
}

void main() {
  testWidgets('the hidden Sessions tab is the same widget when the shell rebuilds, so it is not rebuilt', (WidgetTester tester) async {
    final ProviderContainer container = ProviderContainer(overrides: <Override>[
      sessionsProvider.overrideWith(_Sessions.new),
      connectionStatusProvider.overrideWith(_Status.new),
    ]);
    addTearDown(container.dispose);
    tester.view.physicalSize = const Size(390 * 3, 844 * 3);
    tester.view.devicePixelRatio = 3;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(UncontrolledProviderScope(container: container, child: host(const HomeShell(), screen: true)));
    await tester.pump(const Duration(milliseconds: 400));
    final SessionsScreen before = tester.widget<SessionsScreen>(find.byType(SessionsScreen, skipOffstage: false));
    for (final int tab in <int>[1, 3, 0, 1]) {
      container.read(activeTabProvider.notifier).show(tab);
      await tester.pump(const Duration(milliseconds: 16));
      expect(
        identical(tester.widget<SessionsScreen>(find.byType(SessionsScreen, skipOffstage: false)), before),
        isTrue,
        reason: 'tab $tab',
      );
    }
  });

  testWidgets('the four tabs stay: Sessions, Chat, Flow and Inbox', (WidgetTester tester) async {
    final ProviderContainer container = ProviderContainer(overrides: <Override>[
      sessionsProvider.overrideWith(_Sessions.new),
      connectionStatusProvider.overrideWith(_Status.new),
    ]);
    addTearDown(container.dispose);
    tester.view.physicalSize = const Size(390 * 3, 844 * 3);
    tester.view.devicePixelRatio = 3;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(UncontrolledProviderScope(container: container, child: host(const HomeShell(), screen: true)));
    await tester.pump(const Duration(milliseconds: 400));
    for (final String label in <String>['Sessions', 'Chat', 'Visualize', 'Inbox']) {
      expect(find.text(label), findsWidgets, reason: label);
    }
  });
}
