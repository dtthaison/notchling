import 'dart:async';
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:agent_console/app/widgets/common.dart';
import 'package:agent_console/core/net/bridge_client.dart';
import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/chat/chat_cache.dart';
import 'package:agent_console/features/chat/chat_pictures.dart';
import 'package:agent_console/features/chat/markup_editor.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:flutter_test/flutter_test.dart';

import 'support/harness.dart';

Future<Uint8List> _png(int width, int height) async {
  final ui.PictureRecorder recorder = ui.PictureRecorder();
  Canvas(recorder).drawRect(
    Rect.fromLTWH(0, 0, width.toDouble(), height.toDouble()),
    Paint()..color = const Color(0xFF336699),
  );
  final ui.Image image = await recorder.endRecording().toImage(width, height);
  final ByteData? data = await image.toByteData(format: ui.ImageByteFormat.png);
  image.dispose();
  return data!.buffer.asUint8List();
}

class _StagedLoader extends ChatPictureLoader {
  _StagedLoader(this.thumb, this.sharp) : super(MemoryChatCache(), () => null);
  final Uint8List thumb;
  final Uint8List sharp;
  Completer<Uint8List> sharpAnswer = Completer<Uint8List>();
  int sharpAsks = 0;

  @override
  Future<Uint8List> load(
    String sessionId,
    ChatImage image, {
    int width = thumbnailWidth,
  }) {
    if (width == viewerWidth) {
      sharpAsks++;
      return sharpAnswer.future;
    }
    return Future<Uint8List>.value(thumb);
  }
}

void main() {
  const ChatImage image = ChatImage(
    id: 'p-0',
    mediaType: 'image/png',
    width: 3200,
    height: 2400,
    bytes: 10,
  );

  late Uint8List thumb;
  late Uint8List sharp;

  Future<_StagedLoader> openViewer(WidgetTester tester) async {
    await tester.runAsync(() async {
      thumb = await _png(48, 36);
      sharp = await _png(160, 120);
    });
    final _StagedLoader loader = _StagedLoader(thumb, sharp);
    await tester.pumpWidget(
      ProviderScope(
        overrides: <Override>[chatPictureLoaderProvider.overrideWithValue(loader)],
        child: host(
          PictureViewer(
            sessionId: 'c',
            images: const <ChatImage>[image],
            preview: thumb,
          ),
          screen: true,
        ),
      ),
    );
    await tester.pump();
    await tester.pump();
    return loader;
  }

  VoidCallback? markup(WidgetTester tester) => tester
      .widget<SecondaryButton>(find.byKey(const ValueKey<String>('picture-markup')))
      .onPressed;

  testWidgets('Mark up waits for the sharp picture and then edits that one, not the thumbnail', (
    WidgetTester tester,
  ) async {
    final _StagedLoader loader = await openViewer(tester);
    expect(find.byType(Image), findsOneWidget);
    expect(find.byType(LinearProgressIndicator), findsOneWidget);
    expect(markup(tester), isNull);

    loader.sharpAnswer.complete(loader.sharp);
    await tester.pump();
    await tester.pump();
    expect(find.byType(LinearProgressIndicator), findsNothing);
    expect(markup(tester), isNotNull);

    await tester.tap(find.byKey(const ValueKey<String>('picture-markup')));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 400));
    final MarkupEditor editor = tester.widget<MarkupEditor>(find.byType(MarkupEditor));
    expect(identical(editor.bytes, loader.sharp), isTrue);
    expect(identical(editor.bytes, loader.thumb), isFalse);
  });

  testWidgets('when the sharp picture fails Mark up stays off and Retry brings it back', (
    WidgetTester tester,
  ) async {
    final _StagedLoader loader = await openViewer(tester);
    loader.sharpAnswer.completeError(const BridgeException('timeout', ''));
    await tester.pump();
    await tester.pump();
    expect(find.byType(Image), findsOneWidget);
    expect(markup(tester), isNull);
    expect(find.text('Could not load the picture'), findsOneWidget);
    expect(find.byKey(const ValueKey<String>('picture-retry')), findsOneWidget);

    loader.sharpAnswer = Completer<Uint8List>();
    await tester.tap(find.byKey(const ValueKey<String>('picture-retry')));
    await tester.pump();
    expect(loader.sharpAsks, 2);
    expect(find.text('Could not load the picture'), findsNothing);
    expect(markup(tester), isNull);

    loader.sharpAnswer.complete(loader.sharp);
    await tester.pump();
    await tester.pump();
    expect(markup(tester), isNotNull);
  });
}
