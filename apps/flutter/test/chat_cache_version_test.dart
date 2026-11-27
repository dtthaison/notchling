import 'dart:convert';
import 'dart:io';

import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/chat/chat_cache.dart';
import 'package:flutter_test/flutter_test.dart';

ChatMessage reply(String id, String text) => ChatMessage.fromJson(<String, Object?>{'id': id, 'role': 'assistant', 'text': text, 'time': 1000});

File only(Directory root) => Directory('${root.path}/chat_cache').listSync().whereType<File>().single;

void main() {
  late Directory root;
  late DiskChatCache cache;

  setUp(() {
    root = Directory.systemTemp.createTempSync('chat-cache-version');
    cache = DiskChatCache(() async => root);
  });
  tearDown(() => root.deleteSync(recursive: true));

  test('a copy saved now comes back whole', () async {
    final String long = 'w' * 50000;
    await cache.saveMessages('s1', <ChatMessage>[reply('m1', long)]);
    final List<ChatMessage> kept = await cache.loadMessages('s1');
    expect(kept.map((ChatMessage m) => m.id), <String>['m1']);
    expect(kept.single.text, long);
  });

  test('a cache with no version is discarded, so the cut copy is fetched again', () async {
    await cache.saveMessages('s1', <ChatMessage>[reply('m1', 'cut…')]);
    final File file = only(root);
    await file.writeAsString(jsonEncode(<Object?>[reply('m1', 'cut…').toJson()]));
    expect(await cache.loadMessages('s1'), isEmpty);
    expect(file.existsSync(), isFalse, reason: 'the old copy is dropped, not left to be read again');
  });

  test('a cache of another version is discarded', () async {
    await cache.saveMessages('s1', <ChatMessage>[reply('m1', 'old')]);
    final File file = only(root);
    await file.writeAsString(jsonEncode(<String, Object?>{'version': 1, 'messages': <Object?>[reply('m1', 'old').toJson()]}));
    expect(await cache.loadMessages('s1'), isEmpty);
    expect(file.existsSync(), isFalse);
  });

  test('a cache that cannot be read is empty', () async {
    await cache.saveMessages('s1', <ChatMessage>[reply('m1', 'x')]);
    await only(root).writeAsString('{not json');
    expect(await cache.loadMessages('s1'), isEmpty);
  });
}
