import 'package:agent_console/app/nav_state.dart';
import 'package:agent_console/app/shell.dart';
import 'package:agent_console/core/net/bridge_client.dart';
import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/chat/composer.dart';
import 'package:agent_console/features/connection/connection.dart';
import 'package:agent_console/features/inbox/inbox_screen.dart';
import 'package:agent_console/features/sessions/sessions_notifier.dart';
import 'package:agent_console/features/visualize/viz_providers.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:flutter_test/flutter_test.dart';

import 'support/harness.dart';
import 'support/preview_data.dart';

class _Sessions extends SessionsNotifier {
  @override
  SessionsState build() => SessionsState(
    phase: LoadPhase.ready,
    sessions: <Session>[
      previewSession(
        id: 's1',
        name: 'Phiên 1',
        project: 'app',
        state: SessionState.running,
      ),
    ],
  );
}

class _Status extends ConnectionStatusNotifier {
  @override
  ConnectionStatus build() => const ConnectionStatus(LinkPhase.connected);
}

void main() {
  Future<ProviderContainer> openWide(WidgetTester tester) async {
    final ProviderContainer container = ProviderContainer(
      overrides: <Override>[
        sessionsProvider.overrideWith(_Sessions.new),
        connectionStatusProvider.overrideWith(_Status.new),
        vizProvider.overrideWith(
          (Ref ref, String sessionId) => <String, Object?>{},
        ),
      ],
    );
    addTearDown(container.dispose);
    container.read(selectedSessionIdProvider.notifier).select('s1');
    tester.view.physicalSize = const Size(1200, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(
      UncontrolledProviderScope(
        container: container,
        child: host(const HomeShell(), screen: true),
      ),
    );
    await tester.pump(const Duration(milliseconds: 400));
    return container;
  }

  Finder draftField() => find.descendant(
    of: find.byType(Composer, skipOffstage: false),
    matching: find.byType(TextField, skipOffstage: false),
  );

  String draft(WidgetTester tester) =>
      tester.widget<TextField>(draftField()).controller?.text ?? '';

  testWidgets('on a wide screen the unsent draft survives a visit to Inbox', (
    WidgetTester tester,
  ) async {
    final ProviderContainer container = await openWide(tester);
    await tester.enterText(draftField(), 'chưa gửi');
    await tester.pump();

    container.read(activeTabProvider.notifier).show(3);
    await tester.pump(const Duration(milliseconds: 400));
    expect(find.byType(InboxScreen), findsOneWidget);
    expect(find.byType(Composer), findsNothing);
    expect(draft(tester), 'chưa gửi');

    container.read(activeTabProvider.notifier).show(1);
    await tester.pump(const Duration(milliseconds: 400));
    expect(find.byType(InboxScreen), findsNothing);
    expect(find.byType(Composer), findsOneWidget);
    expect(draft(tester), 'chưa gửi');
  });

  testWidgets('on a wide screen the same chat stays mounted while Flow is open', (
    WidgetTester tester,
  ) async {
    final ProviderContainer container = await openWide(tester);
    await tester.enterText(draftField(), 'nháp');
    await tester.pump();
    final Element before = tester.element(draftField());

    container.read(activeTabProvider.notifier).show(2);
    await tester.pump(const Duration(milliseconds: 400));
    expect(find.byType(Composer), findsNothing);
    expect(find.byType(Composer, skipOffstage: false), findsOneWidget);

    container.read(activeTabProvider.notifier).show(0);
    await tester.pump(const Duration(milliseconds: 400));
    expect(identical(tester.element(draftField()), before), isTrue);
    expect(draft(tester), 'nháp');
  });
}
