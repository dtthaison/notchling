import 'package:agent_console/app/widgets/kit.dart';
import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/chat/chat_cache.dart';
import 'package:agent_console/features/chat/chat_parts.dart';
import 'package:agent_console/features/chat/chat_pictures.dart';
import 'package:agent_console/features/chat/conversation_view.dart';
import 'package:agent_console/features/chat/chat_notifier.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:flutter_test/flutter_test.dart';

import 'support/harness.dart';

ChatMessage user(String id, String text, {bool queued = false}) =>
    ChatMessage(id: id, role: ChatRole.user, text: text, time: DateTime(2026), queued: queued);

ChatMessage assistant(String id, String text) =>
    ChatMessage(id: id, role: ChatRole.assistant, text: text, time: DateTime(2026));

Widget scoped(Widget child) => ProviderScope(
  overrides: <Override>[chatPictureLoaderProvider.overrideWithValue(ChatPictureLoader(MemoryChatCache(), () => null))],
  child: host(child),
);

Finder bubbleOf(Type type) => find.descendant(of: find.byType(type), matching: find.byType(Surface)).first;

Widget list(List<ChatMessage> messages, {List<PendingSend> pending = const <PendingSend>[], bool working = false}) => scoped(
  SizedBox(
    height: 900,
    child: ConversationList(
      sessionId: 's',
      entries: groupEntries(messages),
      pending: pending,
      hasMore: false,
      loadingOlder: false,
      onLoadOlder: () {},
      onRetry: (String _) {},
      onDiscard: (String _) {},
      working: working,
    ),
  ),
);

void main() {
  group('the sending bubble and its status line', () {
    testWidgets('a sending bubble is as wide as the sent one with the same text', (WidgetTester tester) async {
      await tester.pumpWidget(scoped(
        Column(children: <Widget>[
          PendingBubble(text: 'ok', failed: false, reason: null, onRetry: () {}, onDiscard: () {}),
          UserBubble(message: user('u', 'ok'), sessionId: 's'),
        ]),
      ));
      final Size sending = tester.getSize(bubbleOf(PendingBubble));
      final Size sent = tester.getSize(bubbleOf(UserBubble));
      expect(sending, sent);
      expect(sending.width, lessThan(200));
    });

    testWidgets('sending, waiting and sent are one line under the bubble, never inside it', (WidgetTester tester) async {
      await tester.pumpWidget(scoped(
        Column(children: <Widget>[
          PendingBubble(text: 'one', failed: false, reason: null, onRetry: () {}, onDiscard: () {}),
          UserBubble(message: user('q', 'two', queued: true), sessionId: 's'),
          UserBubble(message: user('u', 'three'), sessionId: 's', sent: true),
        ]),
      ));
      for (final (Type, String) row in <(Type, String)>[(PendingBubble, 'sending'), (UserBubble, 'waiting'), (UserBubble, 'sent')]) {
        final Finder label = find.descendant(of: find.byType(row.$1), matching: find.text(row.$2));
        expect(label, findsWidgets, reason: row.$2);
      }
      final Rect pendingBubble = tester.getRect(bubbleOf(PendingBubble));
      expect(tester.getTopLeft(find.text('sending')).dy, greaterThanOrEqualTo(pendingBubble.bottom));
      final Rect userBubbles = tester.getRect(find.descendant(of: find.byType(UserBubble).first, matching: find.byType(Surface)).first);
      expect(tester.getTopLeft(find.text('waiting')).dy, greaterThanOrEqualTo(userBubbles.bottom));
      final Rect lastBubble = tester.getRect(find.descendant(of: find.byType(UserBubble).last, matching: find.byType(Surface)).first);
      expect(tester.getTopLeft(find.text('sent')).dy, greaterThanOrEqualTo(lastBubble.bottom));
    });

    testWidgets('sent goes under your newest message only', (WidgetTester tester) async {
      await tester.pumpWidget(list(<ChatMessage>[user('a', 'first'), assistant('b', 'reply'), user('c', 'second')]));
      await tester.pump(const Duration(milliseconds: 400));
      expect(find.text('sent'), findsOneWidget);
      final double sentY = tester.getTopLeft(find.text('sent')).dy;
      expect(sentY, greaterThan(tester.getTopLeft(find.text('second')).dy));
    });

    testWidgets('no sent line while another message is still sending', (WidgetTester tester) async {
      await tester.pumpWidget(list(
        <ChatMessage>[user('a', 'first')],
        pending: <PendingSend>[PendingSend(id: 'p', text: 'next', key: 'k', time: DateTime(2026))],
      ));
      await tester.pump(const Duration(milliseconds: 400));
      expect(find.text('sent'), findsNothing);
      expect(find.text('sending'), findsOneWidget);
    });
  });

  group('rows are keyed', () {
    testWidgets('a message arriving at the bottom keeps the state of the rows already there', (WidgetTester tester) async {
      final List<ChatMessage> before = <ChatMessage>[assistant('a', 'one'), assistant('b', 'two')];
      await tester.pumpWidget(list(before));
      await tester.pump(const Duration(milliseconds: 400));
      final Element one = tester.element(find.text('one'));
      final State<StatefulWidget> state = tester.state(find.byKey(const ValueKey<String>('a')));
      await tester.pumpWidget(list(<ChatMessage>[...before, assistant('c', 'three')]));
      await tester.pump(const Duration(milliseconds: 400));
      expect(tester.state(find.byKey(const ValueKey<String>('a'))), same(state));
      expect(tester.element(find.text('one')), same(one));
    });

    testWidgets('the working line coming and going keeps the rows', (WidgetTester tester) async {
      final List<ChatMessage> messages = <ChatMessage>[assistant('a', 'one'), assistant('b', 'two')];
      await tester.pumpWidget(list(messages));
      await tester.pump(const Duration(milliseconds: 400));
      final State<StatefulWidget> state = tester.state(find.byKey(const ValueKey<String>('a')));
      await tester.pumpWidget(list(messages, working: true));
      await tester.pump(const Duration(milliseconds: 400));
      expect(tester.state(find.byKey(const ValueKey<String>('a'))), same(state));
    });

    testWidgets('the list asks for a way to find a row again', (WidgetTester tester) async {
      await tester.pumpWidget(list(<ChatMessage>[assistant('a', 'one')]));
      final ListView view = tester.widget(find.byType(ListView));
      expect(view.childrenDelegate, isA<SliverChildBuilderDelegate>());
      expect((view.childrenDelegate as SliverChildBuilderDelegate).findChildIndexCallback, isNotNull);
    });
  });

  group('a tool shows its pictures', () {
    const ChatImage shot = ChatImage(id: 'img-1', mediaType: 'image/png', width: 40, height: 30, bytes: 10);
    final ChatMessage tool = ChatMessage(
      id: 't',
      role: ChatRole.tools,
      text: 'Screenshot',
      time: DateTime(2026),
      toolSummary: 'Screenshot page',
      images: const <ChatImage>[shot],
    );

    testWidgets('under the tool row, outside the fold', (WidgetTester tester) async {
      await tester.pumpWidget(scoped(ToolGroupView(entry: ToolGroupEntry(<ChatMessage>[tool]), sessionId: 's')));
      await tester.pump();
      expect(find.byType(PictureThumbnail), findsOneWidget);
      final double rowBottom = tester.getRect(find.byType(Surface).first).bottom;
      expect(tester.getTopLeft(find.byType(PictureThumbnail)).dy, greaterThanOrEqualTo(rowBottom));
    });

    testWidgets('in the conversation, with the session of the chat', (WidgetTester tester) async {
      await tester.pumpWidget(list(<ChatMessage>[tool]));
      await tester.pump(const Duration(milliseconds: 400));
      expect(find.byType(PictureThumbnail), findsOneWidget);
      expect(tester.widget<PictureThumbnail>(find.byType(PictureThumbnail)).sessionId, 's');
    });

    testWidgets('a tool without pictures draws none', (WidgetTester tester) async {
      final ChatMessage plain = ChatMessage(id: 'p', role: ChatRole.tools, text: 'ls', time: DateTime(2026), toolSummary: 'ls');
      await tester.pumpWidget(scoped(ToolGroupView(entry: ToolGroupEntry(<ChatMessage>[plain]), sessionId: 's')));
      expect(find.byType(PictureThumbnail), findsNothing);
    });
  });
}
