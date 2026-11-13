import 'dart:convert';
import 'dart:io';

import 'package:agent_console/app/widgets/common.dart';
import 'package:agent_console/app/widgets/glass_nav.dart';
import 'package:agent_console/core/protocol/generated.dart' as wire;
import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/connection/connection.dart';
import 'package:agent_console/features/inbox/inbox_notifier.dart';
import 'package:agent_console/features/inbox/inbox_screen.dart';
import 'package:agent_console/core/net/bridge_client.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/fake_ws.dart';
import 'support/harness.dart';

Map<String, Object?> fixture(String name) =>
    (jsonDecode(File('../../protocol/fixtures/$name.json').readAsStringSync()) as Map<Object?, Object?>).cast<String, Object?>();

Map<String, Object?> item(
  String id, {
  String kind = 'secret_approval',
  String state = 'open',
  List<Map<String, Object?>>? actions,
  String? answer,
  String? answeredBy,
  int createdMs = 1793541465000,
  String title = 'Allow Secret',
}) =>
    <String, Object?>{
      'id': id,
      'kind': kind,
      'title': title,
      'detail': 'claude wants the npm token',
      'createdMs': createdMs,
      'actions': actions ??
          <Map<String, Object?>>[
            <String, Object?>{'id': 'allow_once', 'label': 'Allow Once', 'primary': true},
            <String, Object?>{'id': 'allow_always', 'label': 'Always Allow', 'primary': false},
            <String, Object?>{'id': 'deny', 'label': 'Deny', 'primary': false},
          ],
      'state': state,
      'answer': ?answer,
      'answeredBy': ?answeredBy,
      'seen': false,
    };

Map<String, Object?> secretRequest(String id) => item(
      id,
      kind: 'secret_request',
      title: 'Secret Needed',
      actions: <Map<String, Object?>>[
        <String, Object?>{'id': 'open', 'label': 'Open Form', 'primary': true},
        <String, Object?>{'id': 'cancel', 'label': 'Cancel', 'primary': false},
      ],
    );

void main() {
  test('the models read the inbox fixtures as the generated types do', () {
    final Map<String, Object?> list = fixture('InboxListResponse');
    final Map<String, Object?> data = list['data']! as Map<String, Object?>;
    final Map<String, Object?> raw = (data['items']! as List<Object?>).first! as Map<String, Object?>;
    final InboxItem model = InboxItem.fromJson(raw);
    final wire.InboxItem generated = wire.InboxItem.fromJson(raw);
    expect(model.id, generated.id);
    expect(model.kind, generated.kind);
    expect(model.state, generated.state);
    expect(model.isOpen, isTrue);
    expect(model.actions.map((InboxAction a) => a.id), generated.actions.map((wire.InboxAction a) => a.id));
    expect(model.actions.first.primary, isTrue);
    expect(model.createdMs, generated.createdMs);
    expect(model.session, generated.source?.session);

    final Map<String, Object?> event = fixture('InboxEvent');
    expect(InboxItem.fromJson(event['item']! as Map<String, Object?>).id, 'vault-approval-1');
  });

  test('an item from a newer Mac with an unknown kind and no actions still reads', () {
    final InboxItem model = InboxItem.fromJson(<String, Object?>{
      'id': 'x',
      'kind': 'from_the_future',
      'title': 'T',
      'detail': '',
      'createdMs': 1,
      'state': 'open',
    });
    expect(model.actions, isEmpty);
    expect(model.seen, isFalse);
  });

  group('the inbox state', () {
    FakeWs mac({
      List<Map<String, Object?>> items = const <Map<String, Object?>>[],
      bool oldMac = false,
      Map<String, Object?> Function(Map<String, Object?> request)? answer,
      String? failAnswer,
    }) {
      return FakeWs(onSend: (FakeWs s, Map<String, Object?> m) {
        switch (m['op']) {
          case 'hello':
            s.reply(m['id']!, data: <String, Object?>{'version': 1});
          case 'sessions.list':
            s.reply(m['id']!, data: <String, Object?>{'sessions': <Object?>[]});
          case 'subscribe':
            s.reply(m['id']!, data: <String, Object?>{'subscribed': true});
          case 'inbox.list':
            if (oldMac) {
              s.fail(m['id']!, 'invalid', 'Không có lệnh "inbox.list".');
            } else {
              s.reply(m['id']!, data: <String, Object?>{'items': items, 'unread': items.length});
            }
          case 'inbox.answer':
            if (failAnswer != null) {
              s.fail(m['id']!, 'invalid', failAnswer);
              return;
            }
            s.reply(m['id']!, data: answer == null ? <String, Object?>{'answered': true, 'state': 'answered', 'answer': m['action']} : answer(m));
        }
      });
    }

    Future<ProviderContainer> open(WidgetTester tester, FakeWs socket) async {
      final ProviderContainer container = containerWith(socket);
      await tester.runAsync(() async {
        await connect(container);
        container.listen<InboxState>(inboxProvider, (InboxState? a, InboxState b) {});
        await until(() => container.read(inboxProvider).phase != InboxPhase.loading);
      });
      return container;
    }

    testWidgets('loads the list on connect and splits Needs You from Done', (WidgetTester tester) async {
      final FakeWs socket = mac(items: <Map<String, Object?>>[
        item('a'),
        item('b', state: 'answered', answer: 'deny', answeredBy: 'mac'),
        item('c', createdMs: 1793541465001),
      ]);
      final ProviderContainer container = await open(tester, socket);
      final InboxState state = container.read(inboxProvider);
      expect(state.phase, InboxPhase.ready);
      expect(state.needsYou.map((InboxItem i) => i.id), <String>['c', 'a']);
      expect(state.done.map((InboxItem i) => i.id), <String>['b']);
      expect(state.openCount, 2);
    });

    testWidgets('a push adds an item, updates it, and a removed push drops it', (WidgetTester tester) async {
      final FakeWs socket = mac(items: <Map<String, Object?>>[item('a')]);
      final ProviderContainer container = await open(tester, socket);

      socket.event('inbox', <String, Object?>{'item': item('n', kind: 'notice')});
      await tester.runAsync(() => until(() => container.read(inboxProvider).items.length == 2));

      socket.event('inbox', <String, Object?>{'item': item('a', state: 'answered', answer: 'deny', answeredBy: 'mac')});
      await tester.runAsync(() => until(() => container.read(inboxProvider).openCount == 1));
      expect(container.read(inboxProvider).done.single.id, 'a');

      socket.event('inbox', <String, Object?>{'removed': 'a'});
      await tester.runAsync(() => until(() => container.read(inboxProvider).items.length == 1));
      expect(container.read(inboxProvider).items.single.id, 'n');
    });

    testWidgets('the subscribe asks for the inbox', (WidgetTester tester) async {
      final FakeWs socket = mac();
      final ProviderContainer container = await open(tester, socket);
      await tester.runAsync(() async {
        final BridgeClient client = container.read(bridgeClientProvider)!;
        await client.subscribe(const SubscriptionSpec(sessions: true, inbox: true));
      });
      expect(socket.sent.lastWhere((Map<String, Object?> m) => m['op'] == 'subscribe'), containsPair('inbox', true));
      expect(const SubscriptionSpec(sessions: true).toParams().containsKey('inbox'), isFalse);
    });

    testWidgets('an older Mac that does not know inbox.list leaves the tab empty and quiet', (WidgetTester tester) async {
      final ProviderContainer container = await open(tester, mac(oldMac: true));
      expect(container.read(inboxProvider).phase, InboxPhase.unsupported);
      expect(container.read(inboxProvider).openCount, 0);
    });

    testWidgets('the screen shows Needs You then Done, with the action buttons', (WidgetTester tester) async {
      final FakeWs socket = mac(items: <Map<String, Object?>>[
        item('a'),
        item('b', state: 'answered', answer: 'deny', answeredBy: 'mac', title: 'Old One'),
      ]);
      final ProviderContainer container = await open(tester, socket);
      await tester.pumpWidget(UncontrolledProviderScope(container: container, child: host(const Scaffold(body: InboxScreen()), locale: const Locale('en'), screen: true)));
      await tester.pump();

      expect(find.text('NEEDS YOU'), findsOneWidget);
      expect(find.text('DONE'), findsOneWidget);
      expect(tester.getTopLeft(find.text('NEEDS YOU')).dy, lessThan(tester.getTopLeft(find.text('DONE')).dy));
      expect(find.text('Allow on Mac'), findsOneWidget);
      expect(find.text('Deny'), findsOneWidget);
      expect(find.text('Old One'), findsOneWidget);
      expect(find.textContaining('Answered on the Mac'), findsOneWidget);
    });

    testWidgets('pressing a button sends inbox.answer and moves the item to Done', (WidgetTester tester) async {
      final FakeWs socket = mac(items: <Map<String, Object?>>[item('a')]);
      final ProviderContainer container = await open(tester, socket);
      await tester.pumpWidget(UncontrolledProviderScope(container: container, child: host(const Scaffold(body: InboxScreen()), locale: const Locale('en'), screen: true)));
      await tester.pump();

      await tester.tap(find.text('Deny'));
      await tester.runAsync(() => until(() => container.read(inboxProvider).openCount == 0));
      await tester.pump();
      final Map<String, Object?> sent = socket.sent.lastWhere((Map<String, Object?> m) => m['op'] == 'inbox.answer');
      expect(sent['itemId'], 'a');
      expect(sent['action'], 'deny');
      expect(container.read(inboxProvider).done.single.answer, 'deny');
      expect(container.read(inboxProvider).done.single.answeredBy, 'phone');
    });

    testWidgets('a secret approval shows Allow on Mac as one disabled hint and Deny works', (WidgetTester tester) async {
      final FakeWs socket = mac(items: <Map<String, Object?>>[item('a')]);
      final ProviderContainer container = await open(tester, socket);
      await tester.pumpWidget(UncontrolledProviderScope(container: container, child: host(const Scaffold(body: InboxScreen()), locale: const Locale('en'), screen: true)));
      await tester.pump();

      expect(find.text('Allow Once'), findsNothing);
      expect(find.text('Always Allow'), findsNothing);
      expect(find.text('Allow on Mac'), findsOneWidget);
      await tester.tap(find.text('Allow on Mac'));
      await tester.pump();
      expect(socket.sent.where((Map<String, Object?> m) => m['op'] == 'inbox.answer'), isEmpty);

      await tester.tap(find.text('Deny'));
      await tester.runAsync(() => until(() => socket.sent.any((Map<String, Object?> m) => m['op'] == 'inbox.answer')));
      expect(socket.sent.lastWhere((Map<String, Object?> m) => m['op'] == 'inbox.answer')['action'], 'deny');
    });

    testWidgets('an item that is not a secret keeps its Allow buttons', (WidgetTester tester) async {
      final FakeWs socket = mac(items: <Map<String, Object?>>[item('n', kind: 'notice')]);
      final ProviderContainer container = await open(tester, socket);
      await tester.pumpWidget(UncontrolledProviderScope(container: container, child: host(const Scaffold(body: InboxScreen()), locale: const Locale('en'), screen: true)));
      await tester.pump();

      expect(find.text('Allow Once'), findsOneWidget);
      expect(find.text('Allow on Mac'), findsNothing);
    });

    testWidgets('when the Mac answered first the phone shows it as done on the Mac', (WidgetTester tester) async {
      final FakeWs socket = mac(
        items: <Map<String, Object?>>[item('a')],
        answer: (Map<String, Object?> m) => <String, Object?>{'answered': false, 'state': 'answered', 'answer': 'allow_once'},
      );
      final ProviderContainer container = await open(tester, socket);
      await tester.pumpWidget(UncontrolledProviderScope(container: container, child: host(const Scaffold(body: InboxScreen()), locale: const Locale('en'), screen: true)));
      await tester.pump();

      await tester.tap(find.text('Deny'));
      await tester.runAsync(() => until(() => container.read(inboxProvider).openCount == 0));
      await tester.pump();
      final InboxItem shown = container.read(inboxProvider).done.single;
      expect(shown.answer, 'allow_once');
      expect(shown.answeredBy, isNot('phone'));
      expect(find.textContaining('Answered on the Mac'), findsOneWidget);
    });

    testWidgets('a secret request shows Open on Mac as a disabled hint and Cancel works', (WidgetTester tester) async {
      final FakeWs socket = mac(items: <Map<String, Object?>>[secretRequest('r')]);
      final ProviderContainer container = await open(tester, socket);
      await tester.pumpWidget(UncontrolledProviderScope(container: container, child: host(const Scaffold(body: InboxScreen()), locale: const Locale('en'), screen: true)));
      await tester.pump();

      expect(find.text('Open Form'), findsNothing);
      expect(find.text('Open on Mac'), findsOneWidget);
      await tester.tap(find.text('Open on Mac'));
      await tester.pump();
      expect(socket.sent.where((Map<String, Object?> m) => m['op'] == 'inbox.answer'), isEmpty);

      await tester.tap(find.text('Cancel'));
      await tester.runAsync(() => until(() => socket.sent.any((Map<String, Object?> m) => m['op'] == 'inbox.answer')));
      expect(socket.sent.lastWhere((Map<String, Object?> m) => m['op'] == 'inbox.answer')['action'], 'cancel');
    });

    Future<void> show(WidgetTester tester, ProviderContainer container) async {
      await tester.pumpWidget(UncontrolledProviderScope(container: container, child: host(const Scaffold(body: InboxScreen()), locale: const Locale('en'), screen: true)));
      await tester.pump();
    }

    testWidgets('an answer the Mac did not take leaves the item open with a short message', (WidgetTester tester) async {
      final FakeWs socket = mac(
        items: <Map<String, Object?>>[item('a')],
        answer: (Map<String, Object?> m) => <String, Object?>{'answered': false, 'state': 'open'},
      );
      final ProviderContainer container = await open(tester, socket);
      await show(tester, container);
      await tester.tap(find.text('Deny'));
      await tester.runAsync(() => until(() => socket.sent.any((Map<String, Object?> m) => m['op'] == 'inbox.answer')));
      await tester.runAsync(() => until(() => container.read(inboxProvider).busy.isEmpty));
      await tester.pump();
      expect(container.read(inboxProvider).openCount, 1);
      expect(find.textContaining("didn't take"), findsOneWidget);
      expect(find.text('Deny'), findsOneWidget);
    });

    testWidgets('an error from the Mac, such as acting being off, is shown on the item', (WidgetTester tester) async {
      const String off = 'Máy này chưa bật quyền điều khiển session.';
      final FakeWs socket = mac(items: <Map<String, Object?>>[item('a')], failAnswer: off);
      final ProviderContainer container = await open(tester, socket);
      await show(tester, container);
      await tester.tap(find.text('Deny'));
      await tester.runAsync(() => until(() => container.read(inboxProvider).errors.containsKey('a')));
      await tester.pump();
      expect(find.text(off), findsOneWidget);
      expect(container.read(inboxProvider).openCount, 1);
      expect(find.text('Deny'), findsOneWidget);
    });

    testWidgets('a reload keeps the items on screen', (WidgetTester tester) async {
      final ProviderContainer container = await open(tester, mac(items: <Map<String, Object?>>[item('a')]));
      late InboxState during;
      await tester.runAsync(() async {
        container.invalidate(inboxProvider);
        during = container.read(inboxProvider);
        await Future<void>.delayed(const Duration(milliseconds: 150));
      });
      expect(during.phase, InboxPhase.ready);
      expect(during.items.single.id, 'a');
    });

    testWidgets('only the first open item has the orange primary button', (WidgetTester tester) async {
      final ProviderContainer container = await open(tester, mac(items: <Map<String, Object?>>[item('a', kind: 'notice'), item('b', kind: 'notice', createdMs: 1793541465001)]));
      await show(tester, container);
      expect(find.byType(PrimaryButton), findsOneWidget);
      expect(find.text('Allow Once'), findsNWidgets(2));
      expect(find.byType(SecondaryButton), findsNWidgets(5));
    });

    testWidgets('an empty inbox says so', (WidgetTester tester) async {
      final ProviderContainer container = await open(tester, mac());
      await tester.pumpWidget(UncontrolledProviderScope(container: container, child: host(const Scaffold(body: InboxScreen()), locale: const Locale('en'), screen: true)));
      await tester.pump();
      expect(find.text('Nothing Needs You'), findsOneWidget);
    });
  });

  group('the tab badge', () {
    Widget nav(int badge) => host(
          GlassNav(selected: 0, onSelect: (int i) {}, inboxCount: badge),
          locale: const Locale('en'),
          screen: true,
        );

    testWidgets('shows the open count on the Inbox tab', (WidgetTester tester) async {
      await tester.pumpWidget(nav(3));
      await tester.pump();
      expect(find.text('Inbox'), findsOneWidget);
      expect(find.byKey(const ValueKey<String>('inbox-badge')), findsOneWidget);
      expect(find.text('3'), findsOneWidget);
    });

    testWidgets('shows no badge at zero and caps a long count', (WidgetTester tester) async {
      await tester.pumpWidget(nav(0));
      await tester.pump();
      expect(find.byKey(const ValueKey<String>('inbox-badge')), findsNothing);
      await tester.pumpWidget(nav(120));
      await tester.pump();
      expect(find.text('99+'), findsOneWidget);
    });
  });
}
