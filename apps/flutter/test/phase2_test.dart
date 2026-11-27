import 'dart:convert';
import 'dart:typed_data';

import 'package:agent_console/core/net/bridge_client.dart';
import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/chat/photo_send.dart';
import 'package:agent_console/features/chat/slash_commands.dart';
import 'package:agent_console/features/connection/connection.dart';
import 'package:agent_console/features/sessions/archive_store.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:flutter_test/flutter_test.dart';

import 'support/fake_ws.dart';
import 'support/harness.dart';
import 'support/preview_data.dart';

void main() {
  group('commands.list', () {
    test('the session\'s real commands replace the built-ins when the Mac answers', () async {
      final FakeWs socket = happyMac(auto: <String, Map<String, Object?>>{
        'commands.list': <String, Object?>{
          'commands': <Object?>[
            <String, Object?>{'name': 'clear', 'description': 'Clear'},
            <String, Object?>{'name': 'sd:approve', 'description': 'Approve a plan', 'argumentHint': '[id]'},
          ],
        },
      });
      final ProviderContainer container = containerWith(socket);
      await connect(container);
      container.listen<List<SlashCommand>>(slashCommandsProvider('s'), (List<SlashCommand>? a, List<SlashCommand> b) {});
      expect(container.read(slashCommandsProvider('s')).length, builtInSlashCommands.length, reason: 'built-ins at once');
      await until(() => container.read(slashCommandsProvider('s')).length == 2);
      expect(container.read(slashCommandsProvider('s')).last.argumentHint, '[id]');
    });

    test('an older Mac that does not know the op keeps the built-ins', () async {
      final FakeWs socket = FakeWs(onSend: (FakeWs s, Map<String, Object?> m) {
        if (m['op'] == 'hello') s.reply(m['id']!);
        if (m['op'] == 'commands.list') s.fail(m['id']!, 'invalid', 'Không có lệnh');
      });
      final ProviderContainer container = containerWith(socket);
      await connect(container);
      container.listen<List<SlashCommand>>(slashCommandsProvider('s'), (List<SlashCommand>? a, List<SlashCommand> b) {});
      await until(() => socket.sent.any((Map<String, Object?> m) => m['op'] == 'commands.list'));
      await Future<void>.delayed(const Duration(milliseconds: 30));
      expect(container.read(slashCommandsProvider('s')).length, builtInSlashCommands.length);
    });
  });

  group('sending a photo', () {
    test('goes in order: begin with the size, base64 pieces of at most 96 KB, end', () async {
      final List<Map<String, Object?>> chunks = <Map<String, Object?>>[];
      final FakeWs socket = FakeWs(onSend: (FakeWs s, Map<String, Object?> m) {
        switch (m['op']) {
          case 'hello':
            s.reply(m['id']!);
          case 'session.image.begin':
            s.reply(m['id']!, data: <String, Object?>{'uploadId': 'up1', 'chunkBytes': 98304});
          case 'session.image.chunk':
            chunks.add(m);
            s.reply(m['id']!, data: <String, Object?>{'received': m['seq']});
          case 'session.image.end':
            s.reply(m['id']!, data: <String, Object?>{'sent': true});
        }
      });
      final ProviderContainer container = containerWith(socket);
      await connect(container);
      final Uint8List photo = Uint8List.fromList(List<int>.generate(250000, (int i) => i % 251));
      final List<double> progress = <double>[];
      await container.read(bridgeClientProvider)!.sessionImage('s1', photo, caption: '  look  ', onProgress: progress.add);
      final Map<String, Object?> begin = socket.sent.firstWhere((Map<String, Object?> m) => m['op'] == 'session.image.begin');
      expect(begin['bytes'], 250000);
      expect(begin['caption'], 'look');
      expect(begin['sessionId'], 's1');
      expect(chunks.map((Map<String, Object?> c) => c['seq']), <Object?>[0, 1, 2]);
      final BytesBuilder joined = BytesBuilder();
      for (final Map<String, Object?> c in chunks) {
        final Uint8List piece = base64Decode(c['data']! as String);
        expect(piece.length, lessThanOrEqualTo(96 * 1024));
        expect((c['data']! as String).length, lessThan(256 * 1024), reason: 'under the bridge\'s message cap');
        joined.add(piece);
      }
      expect(joined.toBytes(), photo);
      expect(progress.last, 1);
      expect(socket.sent.last['op'], 'session.image.end');
    });

    test('a refusal from the Mac comes back as its reason, and nothing more is sent', () async {
      final FakeWs socket = FakeWs(onSend: (FakeWs s, Map<String, Object?> m) {
        if (m['op'] == 'hello') s.reply(m['id']!);
        if (m['op'] == 'session.image.begin') s.fail(m['id']!, 'view_only', 'Session mở ngoài Console — chỉ xem.');
      });
      final ProviderContainer container = containerWith(socket);
      await connect(container);
      await expectLater(
        container.read(bridgeClientProvider)!.sessionImage('s1', Uint8List(10)),
        throwsA(isA<BridgeException>().having((BridgeException e) => e.isViewOnly, 'isViewOnly', isTrue)),
      );
      expect(socket.sent.where((Map<String, Object?> m) => '${m['op']}'.startsWith('session.image.chunk')), isEmpty);
    });

    testWidgets('the preview shows progress while sending and the reason when the Mac refuses', (WidgetTester tester) async {
      bool fail = true;
      final List<String> captions = <String>[];
      await tester.pumpWidget(host(PhotoPreview(
        bytes: Uint8List.fromList(<int>[
          0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
          0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xFF, 0xFF, 0x3F,
          0x00, 0x05, 0xFE, 0x02, 0xFE, 0xA7, 0x35, 0x81, 0x84, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ]),
        send: (String caption, void Function(double) onProgress) async {
          captions.add(caption);
          onProgress(0.5);
          if (fail) throw const BridgeException('view_only', 'Chỉ xem');
        },
      ), screen: true));
      await tester.enterText(find.byType(TextField), 'mô tả');
      await tester.tap(find.bySemanticsLabel('Send'));
      await tester.pump();
      await tester.pump();
      expect(captions, <String>['mô tả']);
      expect(find.text('This session is view only.'), findsOneWidget);
      expect(find.byType(LinearProgressIndicator), findsNothing, reason: 'progress goes away after a failure');
      fail = false;
      await tester.tap(find.bySemanticsLabel('Send'));
      await tester.pump();
      expect(captions.length, 2, reason: 'sending again is possible');
    });
  });

  group('archive', () {
    test('survives a restart, and a session that moved since comes back', () async {
      final MemoryArchiveStore store = MemoryArchiveStore();
      final ProviderContainer first = ProviderContainer(overrides: <Override>[archiveStoreProvider.overrideWithValue(store)]);
      addTearDown(first.dispose);
      first.listen<Map<String, int>>(archivedSessionsProvider, (Map<String, int>? a, Map<String, int> b) {});
      first.read(archivedSessionsProvider.notifier).archive('s3');
      await Future<void>.delayed(Duration.zero);
      expect(store.saved.keys, <String>['s3']);

      final ProviderContainer second = ProviderContainer(overrides: <Override>[archiveStoreProvider.overrideWithValue(store)]);
      addTearDown(second.dispose);
      second.listen<Map<String, int>>(archivedSessionsProvider, (Map<String, int>? a, Map<String, int> b) {});
      await until(() => second.read(archivedSessionsProvider).containsKey('s3'));

      final List<Session> sessions = previewSessions();
      final Map<String, int> later = <String, int>{'s3': previewNow.add(const Duration(hours: 1)).millisecondsSinceEpoch};
      expect(hiddenSessionIds(later, sessions), <String>{'s3'});
      final Map<String, int> early = <String, int>{'s3': previewNow.subtract(const Duration(hours: 1)).millisecondsSinceEpoch};
      expect(hiddenSessionIds(early, sessions), isEmpty);
      second.read(archivedSessionsProvider.notifier).restore('s3');
      await Future<void>.delayed(Duration.zero);
      expect(store.saved, isEmpty);
    });
  });
}
