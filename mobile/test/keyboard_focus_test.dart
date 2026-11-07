import 'package:agent_console/app/shell.dart';
import 'package:agent_console/app/nav_state.dart';
import 'package:agent_console/core/net/bridge_client.dart';
import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/chat/chat_header.dart';
import 'package:agent_console/features/chat/chat_screen.dart';
import 'package:agent_console/features/connection/connection.dart';
import 'package:agent_console/features/sessions/sessions_notifier.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:agent_console/features/chat/chat_notifier.dart';
import 'package:agent_console/features/chat/composer.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:flutter_test/flutter_test.dart';

import 'support/harness.dart';
import 'support/preview_data.dart';

void main() {
  Widget composer(TextEditingController controller, FocusNode focus, {double inset = 0, ValueNotifier<double>? insets}) {
    final KeyboardLiftController lift = KeyboardLiftController();
    addTearDown(lift.dispose);
    final Widget field = KeyboardLift(
      controller: lift,
      list: const SizedBox.expand(),
      composer: Composer(
        controller: controller,
        send: const SendIdle(),
        viewOnly: false,
        onSend: () {},
        onRetry: () {},
        onDismissError: () {},
        onMic: () {},
        focusNode: focus,
      ),
    );
    return host(insets == null
        ? SizedBox(height: 600, child: field)
        : ValueListenableBuilder<double>(
            valueListenable: insets,
            builder: (BuildContext context, double value, Widget? child) => MediaQuery(
              data: MediaQuery.of(context).copyWith(viewInsets: EdgeInsets.only(bottom: value)),
              child: child!,
            ),
            child: SizedBox(height: 600, child: field),
          ));
  }

  testWidgets('tapping the composer field focuses it', (WidgetTester tester) async {
    final TextEditingController controller = TextEditingController();
    final FocusNode focus = FocusNode();
    addTearDown(controller.dispose);
    addTearDown(focus.dispose);
    await tester.pumpWidget(composer(controller, focus));
    expect(focus.hasFocus, isFalse);
    await tester.tap(find.byType(TextField));
    await tester.pump();
    expect(focus.hasFocus, isTrue);
  });

  testWidgets('the keyboard coming up does not rebuild the composer and drop its focus', (WidgetTester tester) async {
    final TextEditingController controller = TextEditingController();
    final FocusNode focus = FocusNode();
    final ValueNotifier<double> insets = ValueNotifier<double>(0);
    addTearDown(controller.dispose);
    addTearDown(focus.dispose);
    addTearDown(insets.dispose);
    await tester.pumpWidget(composer(controller, focus, insets: insets));

    await tester.tap(find.byType(TextField));
    await tester.pump();
    expect(focus.hasFocus, isTrue);

    // The keyboard animates in over several frames.
    for (final double value in <double>[40, 160, 320, 0, 320]) {
      insets.value = value;
      await tester.pump(const Duration(milliseconds: 16));
      expect(focus.hasFocus, isTrue, reason: 'focus survives inset $value');
    }
    await tester.enterText(find.byType(TextField), 'chạy test');
    expect(controller.text, 'chạy test');
  });

  testWidgets('the field takes focus again after a dictation state change', (WidgetTester tester) async {
    final TextEditingController controller = TextEditingController();
    final FocusNode focus = FocusNode();
    addTearDown(controller.dispose);
    addTearDown(focus.dispose);
    await tester.pumpWidget(composer(controller, focus));
    await tester.tap(find.byType(TextField));
    await tester.pump();
    focus.unfocus();
    await tester.pump();
    expect(focus.hasFocus, isFalse);
    await tester.tap(find.byType(TextField));
    await tester.pump();
    expect(focus.hasFocus, isTrue);
  });

  group('session pushes', () {
    Session sessionWith({SessionControl control = SessionControl.console}) => previewSession(
          id: 's1',
          name: 'Phiên 1',
          project: 'app',
          state: SessionState.running,
          control: control,
          activity: 'Đang đọc file',
        );

    Future<ProviderContainer> openChat(WidgetTester tester, {SessionControl control = SessionControl.console}) async {
      final ProviderContainer container = ProviderContainer(overrides: <Override>[
        sessionsProvider.overrideWith(_FakeSessions.new),
      ]);
      addTearDown(container.dispose);
      container.read(sessionsProvider.notifier);
      (container.read(sessionsProvider.notifier) as _FakeSessions).push(<Session>[sessionWith(control: control)]);
      container.read(selectedSessionIdProvider.notifier).select('s1');
      await tester.pumpWidget(UncontrolledProviderScope(
        container: container,
        child: host(const Scaffold(body: ChatScreen()), screen: true),
      ));
      await tester.pump(const Duration(milliseconds: 50));
      return container;
    }

    testWidgets('the same list pushed again does not rebuild the composer', (WidgetTester tester) async {
      final ProviderContainer container = await openChat(tester);
      final _FakeSessions sessions = container.read(sessionsProvider.notifier) as _FakeSessions;
      final Composer composerBefore = tester.widget<Composer>(find.byType(Composer));
      for (int push = 0; push < 5; push++) {
        sessions.push(<Session>[sessionWith()]);
        await tester.pump();
      }
      // The same Composer widget instance means its parent did not build again.
      expect(identical(tester.widget<Composer>(find.byType(Composer)), composerBefore), isTrue);
    });

    testWidgets('a changed canMessage still reaches the composer', (WidgetTester tester) async {
      final ProviderContainer container = await openChat(tester);
      final _FakeSessions sessions = container.read(sessionsProvider.notifier) as _FakeSessions;
      expect(tester.widget<Composer>(find.byType(Composer)).viewOnly, isFalse);
      sessions.push(<Session>[sessionWith(control: SessionControl.viewOnly)]);
      await tester.pump();
      expect(tester.widget<Composer>(find.byType(Composer)).viewOnly, isTrue);
      sessions.push(<Session>[sessionWith()]);
      await tester.pump();
      expect(tester.widget<Composer>(find.byType(Composer)).viewOnly, isFalse);
    });

    testWidgets('only the header follows the session, and it follows it', (WidgetTester tester) async {
      final ProviderContainer container = await openChat(tester);
      final _FakeSessions sessions = container.read(sessionsProvider.notifier) as _FakeSessions;
      expect(tester.widget<ChatHeaderView>(find.byType(ChatHeaderView)).session.activity, 'Đang đọc file');
      sessions.push(<Session>[
        previewSession(id: 's1', name: 'Phiên 1', project: 'app', state: SessionState.running, activity: 'Đang sửa'),
      ]);
      await tester.pump();
      expect(tester.widget<ChatHeaderView>(find.byType(ChatHeaderView)).session.activity, 'Đang sửa');
    });

    testWidgets('the connection banner sits above the chat header, not over it', (WidgetTester tester) async {
      final ProviderContainer container = ProviderContainer(overrides: <Override>[
        sessionsProvider.overrideWith(_FakeSessions.new),
        connectionStatusProvider.overrideWith(_FakeStatus.new),
      ]);
      addTearDown(container.dispose);
      (container.read(sessionsProvider.notifier) as _FakeSessions).push(<Session>[sessionWith()]);
      container.read(selectedSessionIdProvider.notifier).select('s1');
      container.read(activeTabProvider.notifier).show(1);
      tester.view.physicalSize = const Size(390 * 3, 844 * 3);
      tester.view.devicePixelRatio = 3;
      addTearDown(tester.view.reset);
      await tester.pumpWidget(UncontrolledProviderScope(
        container: container,
        child: host(const HomeShell(), screen: true),
      ));
      await tester.pump(const Duration(milliseconds: 400));
      final Rect banner = tester.getRect(find.byType(ConnectionBanner));
      final Rect header = tester.getRect(find.byType(ChatHeaderView));
      expect(banner.height, greaterThan(0));
      expect(banner.bottom, lessThanOrEqualTo(header.top));
    });
  });
}

class _FakeSessions extends SessionsNotifier {
  @override
  SessionsState build() => const SessionsState(phase: LoadPhase.ready);

  void push(List<Session> sessions) => state = SessionsState(phase: LoadPhase.ready, sessions: sessions);
}

class _FakeStatus extends ConnectionStatusNotifier {
  @override
  ConnectionStatus build() => const ConnectionStatus(LinkPhase.disconnected);
}
