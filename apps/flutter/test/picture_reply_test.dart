import 'dart:async';
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:agent_console/app/widgets/common.dart';
import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/chat/chat_cache.dart';
import 'package:agent_console/features/chat/chat_pictures.dart';
import 'package:agent_console/features/chat/photo_send.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/fake_ws.dart';
import 'support/harness.dart';

Future<Uint8List> _png(int width, int height) async {
  final ui.PictureRecorder recorder = ui.PictureRecorder();
  Canvas(recorder).drawRect(Rect.fromLTWH(0, 0, width.toDouble(), height.toDouble()), Paint()..color = const Color(0xFF336699));
  final ui.Image image = await recorder.endRecording().toImage(width, height);
  final ByteData? data = await image.toByteData(format: ui.ImageByteFormat.png);
  image.dispose();
  return data!.buffer.asUint8List();
}

class _Loader extends ChatPictureLoader {
  _Loader(this.thumb, this.sharp) : super(MemoryChatCache(), () => null);
  final Uint8List thumb;
  final Uint8List sharp;
  final Completer<Uint8List> sharpAnswer = Completer<Uint8List>();

  @override
  Future<Uint8List> load(String sessionId, ChatImage image, {int width = thumbnailWidth}) =>
      width == viewerWidth ? sharpAnswer.future : Future<Uint8List>.value(thumb);
}

void main() {
  const ChatImage image = ChatImage(id: 'p-0', mediaType: 'image/png', width: 3200, height: 2400, bytes: 10);

  Future<(_Loader, FakeWs)> open(WidgetTester tester) async {
    late Uint8List thumb;
    late Uint8List sharp;
    await tester.runAsync(() async {
      thumb = await _png(48, 36);
      sharp = await _png(160, 120);
    });
    final _Loader loader = _Loader(thumb, sharp);
    final FakeWs socket = FakeWs(onSend: (FakeWs s, Map<String, Object?> m) {
      switch (m['op']) {
        case 'hello':
          s.reply(m['id']!);
        case 'sessions.list':
          s.reply(m['id']!, data: <String, Object?>{'sessions': <Object?>[]});
        case 'subscribe':
          s.reply(m['id']!, data: <String, Object?>{'subscribed': true});
        case 'session.image.begin':
          s.reply(m['id']!, data: <String, Object?>{'uploadId': 'u'});
        default:
          s.reply(m['id']!);
      }
    });
    final ProviderContainer container = containerWith(socket);
    await tester.runAsync(() => connect(container));
    await tester.pumpWidget(
      UncontrolledProviderScope(
        container: container,
        child: ProviderScope(
          overrides: [chatPictureLoaderProvider.overrideWithValue(loader)],
          child: host(
            Builder(
              builder: (BuildContext context) => TextButton(
                onPressed: () => unawaited(
                  Navigator.of(context).push<void>(
                    MaterialPageRoute<void>(
                      builder: (BuildContext route) => PictureViewer(sessionId: 'c', images: const <ChatImage>[image], preview: thumb),
                    ),
                  ),
                ),
                child: const Text('open'),
              ),
            ),
          ),
        ),
      ),
    );
    await tester.tap(find.text('open'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 400));
    return (loader, socket);
  }

  VoidCallback? reply(WidgetTester tester) => tester.widget<SecondaryButton>(find.byKey(const ValueKey<String>('picture-reply'))).onPressed;

  testWidgets('Reply waits for the sharp picture, then previews that one as it is', (WidgetTester tester) async {
    final (_Loader, FakeWs) rig = await open(tester);
    expect(find.text('Reply'), findsOneWidget);
    expect(reply(tester), isNull);

    rig.$1.sharpAnswer.complete(rig.$1.sharp);
    await tester.pump();
    await tester.pump();
    expect(reply(tester), isNotNull);

    await tester.tap(find.byKey(const ValueKey<String>('picture-reply')));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 400));
    final PhotoPreview preview = tester.widget<PhotoPreview>(find.byType(PhotoPreview));
    expect(identical(preview.bytes, rig.$1.sharp), isTrue);
    expect(find.byType(PictureViewer), findsOneWidget);
  });

  testWidgets('sending the reply sends the picture to the session and closes the viewer', (WidgetTester tester) async {
    final (_Loader, FakeWs) rig = await open(tester);
    rig.$1.sharpAnswer.complete(rig.$1.sharp);
    await tester.pump();
    await tester.pump();
    await tester.tap(find.byKey(const ValueKey<String>('picture-reply')));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 400));
    await tester.enterText(find.byType(TextField), 'this one');
    await tester.tap(find.text('Send'));
    await tester.runAsync(() => until(() => rig.$2.sent.any((Map<String, Object?> m) => m['op'] == 'session.image.end')));
    await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 100)));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 400));
    await tester.pump(const Duration(milliseconds: 400));
    final Map<String, Object?> begin = rig.$2.sent.firstWhere((Map<String, Object?> m) => m['op'] == 'session.image.begin');
    expect(begin['sessionId'], 'c');
    expect(begin['caption'], 'this one');
    expect(begin['bytes'], rig.$1.sharp.length);
    expect(find.byType(PictureViewer), findsNothing);
  });
}
