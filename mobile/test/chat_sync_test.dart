import 'dart:typed_data';

import 'package:agent_console/core/net/bridge_client.dart';
import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/chat/chat_cache.dart';
import 'package:agent_console/features/chat/chat_notifier.dart';
import 'package:agent_console/features/chat/chat_parts.dart';
import 'package:agent_console/features/chat/chat_pictures.dart';
import 'package:agent_console/features/connection/connection.dart';
import 'package:agent_console/features/sessions/sessions_notifier.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:flutter_test/flutter_test.dart';

import 'support/fake_ws.dart';
import 'support/harness.dart';

Map<String, Object?> message(String id, String text, {String role = 'user', bool queued = false, double time = 1000, List<Object?> images = const <Object?>[]}) =>
    <String, Object?>{'id': id, 'role': role, 'text': text, 'time': time, if (queued) 'queued': true, if (images.isNotEmpty) 'images': images};

ChatMessage parsed(String id, String text, {bool queued = false, double time = 1000}) => ChatMessage.fromJson(message(id, text, queued: queued, time: time));

void main() {
  /// A Mac that serves [newest] for a plain chat.get and answers `after` with [onAfter].
  FakeWs mac({
    List<Map<String, Object?>> newest = const <Map<String, Object?>>[],
    Map<String, Object?> Function(String after)? onAfter,
    void Function(FakeWs socket, Map<String, Object?> message)? other,
  }) {
    return FakeWs(onSend: (FakeWs s, Map<String, Object?> m) {
      switch (m['op']) {
        case 'hello':
          s.reply(m['id']!);
        case 'sessions.list':
          s.reply(m['id']!, data: <String, Object?>{'sessions': <Object?>[sessionJson('c')]});
        case 'subscribe':
          s.reply(m['id']!, data: <String, Object?>{'subscribed': true});
        case 'chat.get':
          if (m['after'] is String && onAfter != null) {
            s.reply(m['id']!, data: onAfter(m['after']! as String));
          } else {
            s.reply(m['id']!, data: <String, Object?>{'messages': newest, 'hasMore': false});
          }
        default:
          other?.call(s, m);
      }
    });
  }

  Future<ProviderContainer> open(FakeWs socket, {ChatCache? cache}) async {
    final ProviderContainer container = containerWith(socket, cache: cache);
    await connect(container);
    container.listen<SessionsState>(sessionsProvider, (SessionsState? a, SessionsState b) {});
    container.listen<ChatState>(chatProvider('c'), (ChatState? a, ChatState b) {});
    return container;
  }

  group('cache', () {
    test('a chat with kept messages shows them before the Mac answers, then asks only for what is newer', () async {
      final MemoryChatCache cache = MemoryChatCache()..messages['c'] = <ChatMessage>[parsed('m1', 'one'), parsed('m2', 'two')];
      final FakeWs socket = mac(onAfter: (String after) => <String, Object?>{'messages': <Object?>[message('m3', 'three')], 'hasMore': false});
      final ProviderContainer container = await open(socket, cache: cache);
      await until(() => container.read(chatProvider('c')).messages.length == 3);
      expect(container.read(chatProvider('c')).messages.map((ChatMessage m) => m.id), <String>['m1', 'm2', 'm3']);
      final List<Map<String, Object?>> gets = socket.sent.where((Map<String, Object?> m) => m['op'] == 'chat.get').toList();
      expect(gets, hasLength(1), reason: 'no full page when something was kept');
      expect(gets.single, containsPair('after', 'm2'));
      expect(container.read(chatProvider('c')).refreshing, isFalse);
      await until(() => cache.messages['c']!.length == 3);
    });

    test('the cache keeps at most the last 100 messages', () async {
      final MemoryChatCache cache = MemoryChatCache();
      await cache.saveMessages('c', <ChatMessage>[for (int i = 0; i < 130; i++) parsed('m$i', 'text $i')]);
      final List<ChatMessage> kept = await cache.loadMessages('c');
      expect(kept, hasLength(100));
      expect(kept.first.id, 'm30');
      expect(kept.last.id, 'm129');
    });

    test('messages survive the JSON round trip with their pictures and queued flag', () {
      final ChatMessage original = ChatMessage.fromJson(message('a', 'hi', queued: true, images: <Object?>[
        <String, Object?>{'id': 'b-1-0', 'mediaType': 'image/png', 'width': 40, 'height': 30, 'origin': 'user', 'bytes': 12},
      ]));
      final ChatMessage again = ChatMessage.fromJson(original.toJson());
      expect(again.queued, isTrue);
      expect(again.images.single.id, 'b-1-0');
      expect(again.images.single.aspect, closeTo(4 / 3, 0.001));
    });
  });

  group('catching up', () {
    test('a reply that arrived while away is fetched with after and merged by id', () async {
      final List<String> asked = <String>[];
      final FakeWs socket = mac(
        newest: <Map<String, Object?>>[message('m1', 'one'), message('m2', 'two')],
        onAfter: (String after) {
          asked.add(after);
          return <String, Object?>{'messages': <Object?>[message('m3', 'reply'), message('m4', 'more')], 'hasMore': false};
        },
      );
      final ProviderContainer container = await open(socket);
      await until(() => container.read(chatProvider('c')).phase == LoadPhase.ready);
      await container.read(chatProvider('c').notifier).refreshNewest();
      expect(asked, <String>['m2']);
      expect(container.read(chatProvider('c')).messages.map((ChatMessage m) => m.id), <String>['m1', 'm2', 'm3', 'm4']);
      // A second catch-up with nothing new changes nothing and does not duplicate.
      await container.read(chatProvider('c').notifier).refreshNewest();
      expect(container.read(chatProvider('c')).messages, hasLength(4));
    });

    test('reset: when the Mac no longer has the message, its newest page replaces what is shown', () async {
      final FakeWs socket = mac(
        newest: <Map<String, Object?>>[message('m1', 'one')],
        onAfter: (String after) => <String, Object?>{'messages': <Object?>[message('n8', 'eight'), message('n9', 'nine')], 'hasMore': true, 'reset': true},
      );
      final ProviderContainer container = await open(socket);
      await until(() => container.read(chatProvider('c')).phase == LoadPhase.ready);
      await container.read(chatProvider('c').notifier).refreshNewest();
      final ChatState state = container.read(chatProvider('c'));
      expect(state.messages.map((ChatMessage m) => m.id), <String>['n8', 'n9']);
      expect(state.hasMore, isTrue);
    });

    test('subscribe carries the newest message id the phone has', () async {
      final FakeWs socket = mac(newest: <Map<String, Object?>>[message('m1', 'one'), message('m2', 'two')]);
      final ProviderContainer container = await open(socket);
      await until(() => container.read(chatProvider('c')).phase == LoadPhase.ready);
      final BridgeClient client = container.read(bridgeClientProvider)!;
      await client.subscribe(const SubscriptionSpec(sessions: true, sessionId: 'c'));
      expect(socket.sent.lastWhere((Map<String, Object?> m) => m['op'] == 'subscribe'), containsPair('lastMessageId', 'm2'));
      socket.event('chat', <String, Object?>{'sessionId': 'c', 'messages': <Object?>[message('m3', 'three')]});
      await until(() => container.read(chatProvider('c')).messages.length == 3);
      await client.subscribe(const SubscriptionSpec(sessions: true, sessionId: 'c'));
      expect(socket.sent.lastWhere((Map<String, Object?> m) => m['op'] == 'subscribe'), containsPair('lastMessageId', 'm3'));
    });
  });

  group('sending and waiting', () {
    test('a sent message is in the list at once, and goes away when the transcript has it', () async {
      final FakeWs socket = mac(other: (FakeWs s, Map<String, Object?> m) {
        if (m['op'] == 'session.message') s.reply(m['id']!, data: <String, Object?>{'sent': true});
      });
      final ProviderContainer container = await open(socket);
      await until(() => container.read(chatProvider('c')).phase == LoadPhase.ready);
      final ChatNotifier notifier = container.read(chatProvider('c').notifier);
      expect(await notifier.send('  hello  '), isTrue);
      expect(container.read(chatProvider('c')).pending.single.text, 'hello');
      expect(container.read(chatProvider('c')).pending.single.failed, isFalse);
      final double now = DateTime.now().millisecondsSinceEpoch.toDouble();
      socket.event('chat', <String, Object?>{'sessionId': 'c', 'messages': <Object?>[message('u1', 'hello', time: now)]});
      await until(() => container.read(chatProvider('c')).pending.isEmpty);
      expect(container.read(chatProvider('c')).messages.single.text, 'hello');
    });

    test('a refused send stays with its reason and can be sent again', () async {
      int attempts = 0;
      final FakeWs socket = mac(other: (FakeWs s, Map<String, Object?> m) {
        if (m['op'] == 'session.message') {
          attempts++;
          if (attempts == 1) {
            s.fail(m['id']!, 'busy', 'Mac bận');
          } else {
            s.reply(m['id']!, data: <String, Object?>{'sent': true});
          }
        }
      });
      final ProviderContainer container = await open(socket);
      await until(() => container.read(chatProvider('c')).phase == LoadPhase.ready);
      final ChatNotifier notifier = container.read(chatProvider('c').notifier);
      await notifier.send('again');
      await until(() => container.read(chatProvider('c')).pending.single.failed);
      expect(container.read(chatProvider('c')).pending.single.reason, 'Mac bận');
      await notifier.retry(container.read(chatProvider('c')).pending.single.id);
      expect(attempts, 2);
      expect(container.read(chatProvider('c')).pending.single.failed, isFalse);
      notifier.discard(container.read(chatProvider('c')).pending.single.id);
      expect(container.read(chatProvider('c')).pending, isEmpty);
    });

    test('a message waiting in the agent queue shows as waiting, then becomes the normal one without a duplicate', () async {
      final FakeWs socket = mac(newest: <Map<String, Object?>>[message('m1', 'work')]);
      final ProviderContainer container = await open(socket);
      await until(() => container.read(chatProvider('c')).phase == LoadPhase.ready);
      socket.event('chat', <String, Object?>{'sessionId': 'c', 'messages': <Object?>[message('q1', 'also this', queued: true)]});
      await until(() => container.read(chatProvider('c')).messages.length == 2);
      expect(container.read(chatProvider('c')).messages.last.queued, isTrue);
      socket.event('chat', <String, Object?>{'sessionId': 'c', 'messages': <Object?>[message('d1', 'also this')]});
      await until(() => container.read(chatProvider('c')).messages.last.id == 'd1');
      final List<ChatMessage> shown = container.read(chatProvider('c')).messages;
      expect(shown.map((ChatMessage m) => m.id), <String>['m1', 'd1']);
      expect(shown.any((ChatMessage m) => m.queued), isFalse);
    });
  });

  group('pictures', () {
    const ChatImage image = ChatImage(id: 'b-1-0', mediaType: 'image/png', width: 40, height: 30, bytes: 10);
    final Uint8List jpeg = Uint8List.fromList(<int>[0xFF, 0xD8, 1, 2, 3]);

    test('chat.image returns the bytes of the IMG1 frame that carries the request id', () async {
      int? asked;
      final FakeWs socket = mac(other: (FakeWs s, Map<String, Object?> m) {
        if (m['op'] == 'chat.image') {
          asked = m['maxWidth'] as int?;
          s.reply(m['id']!, data: <String, Object?>{'binary': true});
          s.binary('IMG1', m['id']! as int, jpeg);
        }
      });
      final ProviderContainer container = await open(socket);
      final Uint8List bytes = await container.read(bridgeClientProvider)!.chatImage('c', 'b-1-0', maxWidth: 480);
      expect(bytes, jpeg);
      expect(asked, 480);
    });

    test('the loader asks the Mac once for two widgets, keeps the bytes on disk, and then serves from there', () async {
      int fetches = 0;
      final FakeWs socket = mac(other: (FakeWs s, Map<String, Object?> m) {
        if (m['op'] == 'chat.image') {
          fetches++;
          s.reply(m['id']!, data: <String, Object?>{'binary': true});
          s.binary('IMG1', m['id']! as int, jpeg);
        }
      });
      final MemoryChatCache cache = MemoryChatCache();
      final ProviderContainer container = await open(socket, cache: cache);
      final ChatPictureLoader loader = container.read(chatPictureLoaderProvider);
      final List<Uint8List> both = await Future.wait(<Future<Uint8List>>[loader.load('c', image), loader.load('c', image)]);
      expect(both.first, jpeg);
      expect(fetches, 1);
      await until(() => cache.images.isNotEmpty);
      final ChatPictureLoader fresh = ChatPictureLoader(cache, () => null);
      expect(await fresh.load('c', image), jpeg, reason: 'offline, from the phone disk');
      expect(fetches, 1);
    });

    test('a failed fetch is an error to retry, not a crash', () async {
      final FakeWs socket = mac(other: (FakeWs s, Map<String, Object?> m) {
        if (m['op'] == 'chat.image') s.fail(m['id']!, 'not_found', 'Không thấy ảnh này.');
      });
      final ProviderContainer container = await open(socket);
      await expectLater(container.read(chatPictureLoaderProvider).load('c', image), throwsA(isA<BridgeException>()));
    });

    testWidgets('the thumbnail shows its shape while loading, the picture when ready, and a retry when it failed', (WidgetTester tester) async {
      bool fail = true;
      final Uint8List png = Uint8List.fromList(<int>[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
        0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xFF, 0xFF, 0x3F,
        0x00, 0x05, 0xFE, 0x02, 0xFE, 0xA7, 0x35, 0x81, 0x84, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
      ]);
      final ChatPictureLoader loader = _ScriptedLoader(() => fail ? throw const BridgeException('timeout', 'x') : png);
      await tester.pumpWidget(ProviderScope(
        overrides: <Override>[chatPictureLoaderProvider.overrideWithValue(loader)],
        child: host(const PictureThumbnail(sessionId: 'c', image: image)),
      ));
      await tester.pump();
      await tester.pump();
      expect(find.text('Không tải được ảnh'), findsOneWidget);
      expect(find.byType(Image), findsNothing);
      fail = false;
      await tester.tap(find.byType(PictureThumbnail));
      await tester.pump();
      await tester.pump();
      expect(find.byType(Image), findsOneWidget);
      expect(find.text('Không tải được ảnh'), findsNothing);
    });

    testWidgets("a tool group shows its tools' pictures without being opened, and none without a session", (WidgetTester tester) async {
      final ChatPictureLoader loader = _ScriptedLoader(() => throw const BridgeException('timeout', 'x'));
      final ToolGroupEntry group = ToolGroupEntry(<ChatMessage>[
        ChatMessage(id: 't1', role: ChatRole.tools, text: '', time: DateTime(2026), toolSummary: 'Read shot.png', images: const <ChatImage>[image]),
        ChatMessage(id: 't2', role: ChatRole.tools, text: '', time: DateTime(2026), toolSummary: 'Bash ls'),
      ]);
      Widget view({String? sessionId}) => ProviderScope(
            overrides: <Override>[chatPictureLoaderProvider.overrideWithValue(loader)],
            child: host(ToolGroupView(entry: group, sessionId: sessionId)),
          );
      await tester.pumpWidget(view(sessionId: 'c'));
      await tester.pump();
      expect(find.byType(PictureThumbnail), findsOneWidget);
      await tester.pumpWidget(view());
      await tester.pump();
      expect(find.byType(PictureThumbnail), findsNothing);
    });
  });
}

class _ScriptedLoader extends ChatPictureLoader {
  _ScriptedLoader(this._script) : super(MemoryChatCache(), () => null);
  final Uint8List Function() _script;

  @override
  Future<Uint8List> load(String sessionId, ChatImage image, {int width = thumbnailWidth}) async => _script();
}
