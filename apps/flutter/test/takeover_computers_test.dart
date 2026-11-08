// A takeover started on one computer ends on that computer, even when the user picked the other computer's
// session of the same id while it was running (decision 0012).
import 'package:agent_console/features/chat/chat_screen.dart';
import 'package:agent_console/features/macs/macs_notifier.dart';
import 'package:agent_console/features/macs/session_ref.dart';
import 'package:agent_console/features/sessions/sessions_notifier.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/fake_ws.dart';
import 'support/harness.dart';
import 'support/two_computers.dart';

void main() {
  testWidgets('switching computers while a takeover runs files the new session under the computer it ran on', (WidgetTester tester) async {
    final FakeWs a = computerSocket();
    final FakeWs b = computerSocket();
    final ProviderContainer container = pairedComputers(a, b);
    container.read(macsProvider);
    await tester.runAsync(() => until(() => container.read(macsProvider).loaded));

    // B's s1 is open in Chat.
    container.read(selectedSessionProvider.notifier).select(const SessionRef('bbbbbbbbbbbbbbbb', 's1'));
    await tester.pumpWidget(UncontrolledProviderScope(container: container, child: host(const Scaffold(body: ChatScreen()), screen: true)));
    await tester.runAsync(() => until(() => container.read(sessionsProvider).byId('s1') != null));
    await tester.pump(const Duration(milliseconds: 100));

    // Take it over; the confirmation, then the request is on its way to B.
    await tester.tap(find.text('Tiếp quản'));
    await tester.pumpAndSettle();
    await tester.tap(find.descendant(of: find.byType(AlertDialog), matching: find.text('Tiếp quản')));
    await tester.pump();
    await tester.runAsync(() => until(() => b.sent.any((Map<String, Object?> m) => m['op'] == 'session.takeover')));

    // Meanwhile the user opens A's s1 (the same id), which makes A the computer in use.
    container.read(selectedSessionProvider.notifier).select(const SessionRef('aaaaaaaaaaaaaaaa', 's1'));
    await tester.pump(const Duration(milliseconds: 100));
    expect(container.read(macsProvider).activeId, macA.id);

    // B answers: its resumed session is on B.
    final Object requestId = b.sent.firstWhere((Map<String, Object?> m) => m['op'] == 'session.takeover')['id']!;
    b.reply(requestId, data: <String, Object?>{'sessionId': 'resumed-1', 'resumed': true});
    await tester.runAsync(() => until(() => container.read(selectedSessionProvider)?.id == 'resumed-1'));

    expect(container.read(selectedSessionProvider), const SessionRef('bbbbbbbbbbbbbbbb', 'resumed-1'), reason: 'filed under B, not under A');
    expect(container.read(macsProvider).activeId, macB.id);
    expect(a.sent.where((Map<String, Object?> m) => m['op'] == 'session.takeover'), isEmpty);
    await tester.pumpWidget(const SizedBox.shrink());
  });
}
