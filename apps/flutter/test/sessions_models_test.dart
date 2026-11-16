import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/chat/chat_screen.dart';
import 'package:agent_console/features/chat/composer.dart';
import 'package:agent_console/features/sessions/sessions_notifier.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:flutter_test/flutter_test.dart';

import 'support/fake_ws.dart';
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
    models: const <ModelChoice>[
      ModelChoice(id: 'claude-opus-9', name: 'Opus Nine'),
    ],
  );
}

void main() {
  test('an older Mac sends no catalog, so there is none to show', () async {
    final FakeWs socket = macWithSessions(<Map<String, Object?>>[sessionJson('s1')]);
    final ProviderContainer container = containerWith(socket);
    await connect(container);
    container.listen<SessionsState>(sessionsProvider, (SessionsState? a, SessionsState b) {});
    await until(() => container.read(sessionsProvider).phase == LoadPhase.ready);
    expect(container.read(sessionsProvider).models, isNull);
  });

  test("sessions.list brings the Mac's catalog and a pushed list keeps it", () async {
    final FakeWs socket = happyMac(
      auto: <String, Map<String, Object?>>{
        'sessions.list': <String, Object?>{
          'sessions': <Object?>[sessionJson('s1')],
          'models': <Object?>[
            <String, Object?>{'id': 'claude-fable-9', 'name': 'Fable Nine'},
            <String, Object?>{'id': 'claude-opus-9', 'name': 'Opus Nine'},
          ],
        },
        'subscribe': <String, Object?>{'subscribed': true},
        'chat.get': <String, Object?>{'messages': <Object?>[], 'hasMore': false},
      },
    );
    final ProviderContainer container = containerWith(socket);
    await connect(container);
    container.listen<SessionsState>(sessionsProvider, (SessionsState? a, SessionsState b) {});
    await until(() => container.read(sessionsProvider).phase == LoadPhase.ready);
    expect(
      container.read(sessionsProvider).models!.map((ModelChoice m) => m.name),
      <String>['Fable Nine', 'Opus Nine'],
    );

    socket.event('sessions', <String, Object?>{'data': <Object?>[sessionJson('s2')]});
    await until(() => container.read(sessionsProvider).sessions.single.id == 's2');
    expect(
      container.read(sessionsProvider).models!.map((ModelChoice m) => m.id),
      <String>['claude-fable-9', 'claude-opus-9'],
    );
  });

  testWidgets("the chat's composer gets the Mac's catalog", (WidgetTester tester) async {
    final ProviderContainer container = ProviderContainer(
      overrides: <Override>[sessionsProvider.overrideWith(_Sessions.new)],
    );
    addTearDown(container.dispose);
    container.read(selectedSessionIdProvider.notifier).select('s1');
    await tester.pumpWidget(
      UncontrolledProviderScope(
        container: container,
        child: host(const Scaffold(body: ChatScreen()), screen: true),
      ),
    );
    await tester.pump(const Duration(milliseconds: 50));
    expect(
      tester.widget<Composer>(find.byType(Composer)).models!.single.name,
      'Opus Nine',
    );
  });
}
