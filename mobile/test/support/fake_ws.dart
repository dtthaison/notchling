import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:agent_console/core/net/ws_transport.dart';

/// A WebSocket the test drives by hand: [incoming] feeds the client, [sent] records what it wrote.
class FakeWs implements WsConnection {
  FakeWs({this.onSend});

  final StreamController<Object> incoming = StreamController<Object>();
  final List<Map<String, Object?>> sent = <Map<String, Object?>>[];
  final void Function(FakeWs socket, Map<String, Object?> message)? onSend;
  bool closed = false;

  @override
  Stream<Object> get messages => incoming.stream;

  @override
  void sendText(String text) {
    final Map<String, Object?> message = (jsonDecode(text) as Map<Object?, Object?>).cast<String, Object?>();
    sent.add(message);
    onSend?.call(this, message);
  }

  void reply(Object id, {Map<String, Object?> data = const <String, Object?>{}}) =>
      incoming.add(jsonEncode(<String, Object?>{'id': id, 'ok': true, 'data': data}));

  void fail(Object id, String code, [String message = 'lỗi']) =>
      incoming.add(jsonEncode(<String, Object?>{'id': id, 'ok': false, 'error': <String, Object?>{'code': code, 'message': message}}));

  void event(String name, Map<String, Object?> fields) => incoming.add(jsonEncode(<String, Object?>{'event': name, ...fields}));

  void binary(String magic, int number, List<int> body) {
    final BytesBuilder builder = BytesBuilder()
      ..add(magic.codeUnits)
      ..add((ByteData(4)..setUint32(0, number)).buffer.asUint8List())
      ..add(body);
    incoming.add(builder.toBytes());
  }

  /// Drops the connection as a network failure would.
  Future<void> drop() => incoming.close();

  @override
  Future<void> close() async {
    closed = true;
    if (!incoming.isClosed) await incoming.close();
  }
}

/// Answers hello (and anything in [auto]) at once, like a happy Mac.
FakeWs happyMac({Map<String, Map<String, Object?>> auto = const <String, Map<String, Object?>>{}}) {
  return FakeWs(onSend: (FakeWs socket, Map<String, Object?> message) {
    final String op = message['op']! as String;
    if (op == 'hello') {
      socket.reply(message['id']!, data: <String, Object?>{'version': 1});
    } else if (auto.containsKey(op)) {
      socket.reply(message['id']!, data: auto[op]!);
    }
  });
}

Map<String, Object?> sessionJson(String id, {String state = 'running', String control = 'console', List<Map<String, Object?>> subagents = const <Map<String, Object?>>[], Map<String, Object?>? pending, Map<String, Object?>? takeover}) => <String, Object?>{
      'id': id,
      'name': 'Session $id',
      'project': 'proj',
      'cwd': '/tmp/proj',
      'entrypoint': 'cli',
      'state': state,
      'control': control,
      'statusUpdatedAt': 1793541465000.0,
      'startedAt': 1793541000000.0,
      'lastActivity': 1793541465000.0,
      'subagents': subagents,
      'pending': ?pending,
      'takeover': ?takeover,
    };
