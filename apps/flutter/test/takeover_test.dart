import 'package:agent_console/core/net/bridge_client.dart';
import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/chat/chat_notifier.dart';
import 'package:agent_console/features/chat/composer.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/fake_ws.dart';
import 'support/harness.dart';

void main() {
  group('Session.takeover', () {
    test('is read from the session JSON, and absent from an older Mac', () {
      expect(Session.fromJson(sessionJson('a', control: 'viewOnly', takeover: <String, Object?>{'available': true})).takeover?.available, isTrue);
      final Session blocked = Session.fromJson(
          sessionJson('b', control: 'viewOnly', takeover: <String, Object?>{'available': false, 'reason': 'Đóng session ở nơi đang mở'}));
      expect(blocked.takeover?.available, isFalse);
      expect(blocked.takeover?.reason, 'Đóng session ở nơi đang mở');
      expect(Session.fromJson(sessionJson('c', control: 'viewOnly')).takeover, isNull);
    });
  });

  group('ChatNotifier.takeOver', () {
    FakeWs mac({String? failCode}) => FakeWs(onSend: (FakeWs socket, Map<String, Object?> message) {
          switch (message['op']) {
            case 'hello':
              socket.reply(message['id']!);
            case 'sessions.list':
              socket.reply(message['id']!, data: <String, Object?>{'sessions': <Object?>[]});
            case 'chat.get':
              socket.reply(message['id']!, data: <String, Object?>{'messages': <Object?>[], 'hasMore': false});
            case 'session.takeover':
              if (failCode != null) {
                socket.fail(message['id']!, failCode, 'Đóng session ở nơi đang mở (Claude desktop / VS Code) rồi tiếp quản');
              } else {
                socket.reply(message['id']!, data: <String, Object?>{'sessionId': 'resumed-1', 'resumed': true});
              }
          }
        });

    test('sends the chat\'s own session id and returns the session to switch to', () async {
      final FakeWs socket = mac();
      final ProviderContainer container = containerWith(socket);
      await connect(container);
      container.listen<ChatState>(chatProvider('old'), (ChatState? a, ChatState b) {});
      final String? newId = await container.read(chatProvider('old').notifier).takeOver();
      expect(newId, 'resumed-1');
      final Map<String, Object?> request = socket.sent.firstWhere((Map<String, Object?> m) => m['op'] == 'session.takeover');
      expect(request['sessionId'], 'old');
      expect(container.read(chatProvider('old')).takeover, isA<TakeoverIdle>());
    });

    test('a refusal keeps the Mac\'s reason and switches nowhere', () async {
      final ProviderContainer container = containerWith(mac(failCode: 'busy'));
      await connect(container);
      container.listen<ChatState>(chatProvider('old'), (ChatState? a, ChatState b) {});
      expect(await container.read(chatProvider('old').notifier).takeOver(), isNull);
      final TakeoverProgress progress = container.read(chatProvider('old')).takeover;
      expect(progress, isA<TakeoverFailed>());
      expect((progress as TakeoverFailed).error.message, contains('Đóng session ở nơi đang mở'));
      container.read(chatProvider('old').notifier).dismissTakeoverError();
      expect(container.read(chatProvider('old')).takeover, isA<TakeoverIdle>());
    });
  });

  group('Composer takeover row', () {
    Future<void> pump(WidgetTester tester, {TakeoverInfo? info, TakeoverProgress progress = const TakeoverIdle(), VoidCallback? onTakeover}) async {
      final TextEditingController controller = TextEditingController();
      addTearDown(controller.dispose);
      await tester.pumpWidget(host(Composer(
        controller: controller,
        send: const SendIdle(),
        viewOnly: true,
        onSend: () {},
        onRetry: () {},
        onDismissError: () {},
        takeover: info,
        takeoverProgress: progress,
        onTakeover: onTakeover,
      )));
    }

    testWidgets('eligible: the dead-end sentence is replaced by a Tiếp quản button that calls back', (WidgetTester tester) async {
      int taps = 0;
      await pump(tester, info: const TakeoverInfo(available: true), onTakeover: () => taps++);
      expect(find.text('Tiếp quản'), findsOneWidget);
      expect(find.textContaining('chỉ xem'), findsOneWidget);
      await tester.tap(find.text('Tiếp quản'));
      expect(taps, 1);
      expect(tester.getSize(find.ancestor(of: find.text('Tiếp quản'), matching: find.byType(Container)).first).height, greaterThanOrEqualTo(48));
    });

    testWidgets('not eligible: shows the Mac\'s reason and no button', (WidgetTester tester) async {
      await pump(tester, info: const TakeoverInfo(available: false, reason: 'Đóng session ở nơi đang mở (Claude desktop / VS Code) rồi tiếp quản'));
      expect(find.textContaining('Đóng session ở nơi đang mở'), findsOneWidget);
      expect(find.text('Tiếp quản'), findsNothing);
    });

    testWidgets('an older Mac without the field keeps the plain view-only sentence', (WidgetTester tester) async {
      await pump(tester);
      expect(find.textContaining('chỉ xem'), findsOneWidget);
      expect(find.text('Tiếp quản'), findsNothing);
    });

    testWidgets('running disables the button; a failure shows its reason', (WidgetTester tester) async {
      int taps = 0;
      await pump(tester, info: const TakeoverInfo(available: true), progress: const TakeoverRunning(), onTakeover: () => taps++);
      expect(find.text('Đang tiếp quản…'), findsOneWidget);
      await tester.tap(find.text('Đang tiếp quản…'));
      expect(taps, 0);

      await pump(tester, info: const TakeoverInfo(available: true), progress: TakeoverFailed(const BridgeException('invalid', 'Thư mục làm việc của session không còn.')), onTakeover: () {});
      expect(find.textContaining('Thư mục làm việc của session không còn.'), findsOneWidget);
    });
  });
}
