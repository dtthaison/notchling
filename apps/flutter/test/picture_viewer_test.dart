import 'dart:typed_data';

import 'package:agent_console/core/net/bridge_client.dart';
import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/chat/chat_cache.dart';
import 'package:agent_console/features/chat/chat_pictures.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:flutter_test/flutter_test.dart';

import 'support/harness.dart';

class _FailingLoader extends ChatPictureLoader {
  _FailingLoader() : super(MemoryChatCache(), () => null);

  @override
  Future<Uint8List> load(String sessionId, ChatImage image, {int width = thumbnailWidth}) async =>
      throw const BridgeException('timeout', 'x');
}

void main() {
  const List<ChatImage> three = <ChatImage>[
    ChatImage(id: 'p-0', mediaType: 'image/png', width: 40, height: 30, bytes: 10),
    ChatImage(id: 'p-1', mediaType: 'image/png', width: 40, height: 30, bytes: 10),
    ChatImage(id: 'p-2', mediaType: 'image/png', width: 40, height: 30, bytes: 10),
  ];

  Future<void> pump(WidgetTester tester, List<ChatImage> images, {int initial = 0}) {
    return tester.pumpWidget(
      ProviderScope(
        overrides: <Override>[chatPictureLoaderProvider.overrideWithValue(_FailingLoader())],
        child: host(
          SizedBox(height: 560, child: PictureViewer(sessionId: 'c', images: images, initial: initial)),
        ),
      ),
    );
  }

  testWidgets('the viewer swipes sideways to the next picture of the message', (WidgetTester tester) async {
    await pump(tester, three, initial: 1);
    await tester.pump();
    expect(find.text('2 / 3'), findsOneWidget);
    await tester.fling(find.byType(PageView), const Offset(-400, 0), 1000);
    await tester.pumpAndSettle();
    expect(find.text('3 / 3'), findsOneWidget);
    await tester.fling(find.byType(PageView), const Offset(400, 0), 1000);
    await tester.pumpAndSettle();
    expect(find.text('2 / 3'), findsOneWidget);
  });

  testWidgets('a single picture shows no counter', (WidgetTester tester) async {
    await pump(tester, <ChatImage>[three.first]);
    await tester.pump();
    expect(find.textContaining(' / '), findsNothing);
  });
}
