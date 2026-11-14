import 'dart:async';
import 'dart:math' as math;
import 'dart:convert';
import 'dart:typed_data';

import '../pairing/mac_profile.dart';
import '../protocol/json.dart';
import '../protocol/models.dart';
import 'tunnel.dart';
import 'ws_transport.dart';

class BridgeException implements Exception {
  const BridgeException(this.code, this.message);
  final String code;
  final String message;

  bool get isViewOnly => code == 'view_only';
  bool get isUnauthorized => code == 'unauthorized';
  bool get isNotFound => code == 'not_found';
  bool get isInvalid => code == 'invalid';

  @override
  String toString() => 'BridgeException($code): $message';
}

enum LinkPhase { connecting, connected, disconnected }

class ConnectionStatus {
  const ConnectionStatus(this.phase, {this.retryAt, this.error, this.host, this.route, this.revoked = false});
  final LinkPhase phase;

  final RouteKind? route;

  final DateTime? retryAt;
  final Object? error;
  final String? host;

  final bool revoked;
}

sealed class BridgeEvent {
  const BridgeEvent();
}

class SessionsEvent extends BridgeEvent {
  const SessionsEvent(this.sessions);
  final List<Session> sessions;
}

class ChatEvent extends BridgeEvent {
  const ChatEvent(this.sessionId, this.messages);
  final String sessionId;
  final List<ChatMessage> messages;
}

class PromptEvent extends BridgeEvent {
  const PromptEvent({this.prompt, this.resolvedRequestId});
  final PendingPrompt? prompt;
  final String? resolvedRequestId;
}

class InboxEvent extends BridgeEvent {
  const InboxEvent({this.item, this.removedId});
  final InboxItem? item;
  final String? removedId;
}

class InboxAnswer {
  const InboxAnswer({required this.answered, required this.state, this.answer});
  final bool answered;
  final String state;
  final String? answer;
}

class OpenEvent extends BridgeEvent {
  const OpenEvent();
}

class PhoneFrame {
  const PhoneFrame(this.sequence, this.jpeg);
  final int sequence;
  final Uint8List jpeg;
}

class SubscriptionSpec {
  const SubscriptionSpec({this.sessions = false, this.inbox = false, this.sessionId, this.subagentId, this.phoneFps, this.phoneWidth, this.phoneWindowId});
  final bool sessions;
  final bool inbox;
  final String? sessionId;
  final String? subagentId;
  final int? phoneFps;
  final int? phoneWidth;
  final int? phoneWindowId;

  bool get wantsPhone => phoneFps != null;

  Json toParams() => <String, Object?>{
        if (sessions) 'sessions': true,
        if (inbox) 'inbox': true,
        'sessionId': ?sessionId,
        'subagentId': ?subagentId,
        if (wantsPhone)
          'phone': <String, Object?>{'fps': phoneFps, 'width': phoneWidth ?? 400, 'deviceWindowId': ?phoneWindowId},
      };
}

const List<Duration> defaultBackoff = <Duration>[
  Duration(seconds: 1),
  Duration(seconds: 2),
  Duration(seconds: 4),
  Duration(seconds: 8),
  Duration(seconds: 16),
  Duration(seconds: 30),
];

class BridgeClient {
  BridgeClient({
    required this.connector,
    required this.profile,
    this.requestTimeout = const Duration(seconds: 15),
    this.backoff = defaultBackoff,
    this.onGoodRoute,
    DateTime Function()? clock,
  }) : _clock = clock ?? DateTime.now;

  final WsConnector connector;
  final MacProfile profile;
  final Duration requestTimeout;
  final List<Duration> backoff;

  final void Function(Endpoint route)? onGoodRoute;
  final DateTime Function() _clock;

  final StreamController<ConnectionStatus> _statusController = StreamController<ConnectionStatus>.broadcast();
  final StreamController<BridgeEvent> _eventController = StreamController<BridgeEvent>.broadcast();
  final StreamController<PhoneFrame> _frameController = StreamController<PhoneFrame>.broadcast();

  WsConnection? _connection;
  bool _stopped = true;
  bool _preferLan = false;
  bool _reconnectNow = false;
  int _nextId = 1;
  final Map<int, Completer<Json>> _pending = <int, Completer<Json>>{};
  final Map<int, Completer<Uint8List>> _snapshotWaiters = <int, Completer<Uint8List>>{};
  final Map<int, Completer<Uint8List>> _imageWaiters = <int, Completer<Uint8List>>{};

  final Map<String, String> _newestChatId = <String, String>{};

  void rememberNewest(String sessionId, String messageId) => _newestChatId[sessionId] = messageId;

  Json _subscribeParams(SubscriptionSpec spec) {
    final String? last = spec.sessionId == null ? null : _newestChatId[spec.sessionId];
    return <String, Object?>{...spec.toParams(), 'lastMessageId': ?last};
  }
  SubscriptionSpec? _subscription;
  Completer<void>? _wake;
  Completer<void>? _closed;
  ConnectionStatus _status = const ConnectionStatus(LinkPhase.disconnected);

  Stream<ConnectionStatus> get status => _statusController.stream;
  Stream<BridgeEvent> get events => _eventController.stream;
  Stream<PhoneFrame> get phoneFrames => _frameController.stream;
  ConnectionStatus get currentStatus => _status;
  bool get isConnected => _status.phase == LinkPhase.connected;

  void _setStatus(ConnectionStatus next) {
    _status = next;
    if (!_statusController.isClosed) _statusController.add(next);
  }

  void start() {
    if (!_stopped) return;
    _stopped = false;
    unawaited(_run());
  }

  Future<void> stop() async {
    _stopped = true;
    _wake?.complete();
    final WsConnection? connection = _connection;
    _connection = null;
    _failPending(const BridgeException('disconnected', ''));
    await connection?.close();
    _setStatus(const ConnectionStatus(LinkPhase.disconnected));
  }

  Future<void> dispose() async {
    await stop();
    await _statusController.close();
    await _eventController.close();
    await _frameController.close();
  }

  void retryNow() => _wake?.complete();

  void networkChanged() {
    if (_stopped) return;
    _preferLan = true;
    switch (_status.phase) {
      case LinkPhase.connecting:
        break;
      case LinkPhase.disconnected:
        retryNow();
      case LinkPhase.connected:
        _reconnectNow = true;
        unawaited(_connection?.close());
    }
  }

  Future<void> _run() async {
    int round = 0;
    while (!_stopped) {
      _setStatus(const ConnectionStatus(LinkPhase.connecting));
      Object? lastError;
      final WsConnection? connection = await _openAndLogin((Object error) => lastError = error);
      if (_stopped) {
        await connection?.close();
        break;
      }
      if (connection != null) {
        round = 0;
        _connection = connection;
        _setStatus(ConnectionStatus(LinkPhase.connected, host: _status.host, route: _status.route));
        unawaited(_resubscribe());
        await _closed!.future;
        _connection = null;
        _failPending(const BridgeException('disconnected', ''));
        if (_stopped) break;
        if (_reconnectNow) {
          _reconnectNow = false;
          continue;
        }
      } else if (lastError case final BridgeException error when error.isUnauthorized) {
        _setStatus(ConnectionStatus(LinkPhase.disconnected, error: error, revoked: true));
        return;
      }
      final Duration delay = backoff[round < backoff.length ? round : backoff.length - 1];
      round++;
      _setStatus(ConnectionStatus(LinkPhase.disconnected, retryAt: _clock().add(delay), error: lastError));
      _wake = Completer<void>();
      await Future.any<void>(<Future<void>>[Future<void>.delayed(delay), _wake!.future]);
      _wake = null;
    }
  }

  Future<WsConnection?> _openAndLogin(void Function(Object error) onError) async {
    final List<Endpoint> routes = profile.endpoints(preferLan: _preferLan);
    for (final Endpoint route in routes) {
      if (_stopped) return null;
      WsConnection? connection;
      try {
        connection = await connector(route);
        _closed = Completer<void>();
        final Completer<void> closedNow = _closed!;
        connection.messages.listen(
          _onMessage,
          onDone: () { if (!closedNow.isCompleted) closedNow.complete(); },
          onError: (Object _) { if (!closedNow.isCompleted) closedNow.complete(); },
          cancelOnError: true,
        );
        _connection = connection;
        await request('hello', <String, Object?>{'token': profile.token}, const Duration(seconds: 8));
        _preferLan = false;
        _setStatus(ConnectionStatus(LinkPhase.connecting, host: route.host, route: route.kind));
        onGoodRoute?.call(route);
        return connection;
      } on Object catch (error) {
        onError(error);
        _connection = null;
        try {
          await connection?.close();
        } on Object catch (_) {
        }
        if (error is BridgeException && error.isUnauthorized) return null;
      }
    }
    return null;
  }

  Future<void> _resubscribe() async {
    final SubscriptionSpec? spec = _subscription;
    if (spec == null) return;
    try {
      await request('subscribe', _subscribeParams(spec));
    } on BridgeException catch (_) {
    }
  }

  void _onMessage(Object message) {
    if (message is String) {
      final Object? decoded;
      try {
        decoded = jsonDecode(message);
      } on FormatException {
        return;
      }
      if (decoded is! Map<String, Object?>) return;
      if (decoded.containsKey('event')) {
        _onEvent(decoded);
      } else if (decoded['id'] is int) {
        _pending.remove(decoded['id'] as int)?.complete(decoded);
      }
    } else if (message is Uint8List) {
      _onBinary(message);
    }
  }

  void _onEvent(Json json) {
    try {
      switch (json['event']) {
        case 'sessions':
          final Object? data = json['data'];
          final List<Session> sessions = (data is List<Object?> ? data : <Object?>[])
              .map((Object? item) => Session.fromJson((item! as Map<Object?, Object?>).cast<String, Object?>()))
              .toList();
          _eventController.add(SessionsEvent(sessions));
        case 'chat':
          final List<ChatMessage> messages = json.objects('messages').map(ChatMessage.fromJson).toList();
          if (messages.isNotEmpty) _newestChatId[json.str('sessionId')] = messages.last.id;
          _eventController.add(ChatEvent(json.str('sessionId'), messages));
        case 'prompt':
          if (json['resolved'] == true) {
            _eventController.add(PromptEvent(resolvedRequestId: json.str('requestId')));
          } else {
            _eventController.add(PromptEvent(prompt: PendingPrompt.fromJson(json.object('prompt'))));
          }
        case 'inbox':
          final Object? removed = json['removed'];
          if (removed is String) {
            _eventController.add(InboxEvent(removedId: removed));
          } else {
            _eventController.add(InboxEvent(item: InboxItem.fromJson(json.object('item'))));
          }
        case 'open':
          _eventController.add(const OpenEvent());
      }
    } on FormatException catch (_) {
    }
  }

  void _onBinary(Uint8List data) {
    if (data.length < 8) return;
    final String magic = String.fromCharCodes(data.sublist(0, 4));
    final int number = ByteData.sublistView(data, 4, 8).getUint32(0);
    final Uint8List body = Uint8List.sublistView(data, 8);
    if (magic == 'PHN1') {
      _frameController.add(PhoneFrame(number, body));
    } else if (magic == 'WEB1') {
      _snapshotWaiters.remove(number)?.complete(body);
    } else if (magic == 'IMG1') {
      _imageWaiters.remove(number)?.complete(body);
    }
  }

  void _failPending(BridgeException error) {
    final List<Completer<Json>> waiting = _pending.values.toList();
    _pending.clear();
    for (final Completer<Json> completer in waiting) {
      if (!completer.isCompleted) completer.completeError(error);
    }
    final List<Completer<Uint8List>> snapshots = <Completer<Uint8List>>[..._snapshotWaiters.values, ..._imageWaiters.values];
    _snapshotWaiters.clear();
    _imageWaiters.clear();
    for (final Completer<Uint8List> completer in snapshots) {
      if (!completer.isCompleted) completer.completeError(error);
    }
  }

  Future<Json> request(String op, [Json params = const <String, Object?>{}, Duration? timeout, int? idOverride]) async {
    final WsConnection? connection = _connection;
    if (connection == null) throw const BridgeException('disconnected', '');
    final int id = idOverride ?? _nextId++;
    final Completer<Json> completer = Completer<Json>();
    _pending[id] = completer;
    connection.sendText(jsonEncode(<String, Object?>{'id': id, 'op': op, ...params}));
    final Json response;
    try {
      response = await completer.future.timeout(timeout ?? requestTimeout);
    } on TimeoutException {
      _pending.remove(id);
      throw const BridgeException('timeout', '');
    }
    if (response['ok'] == true) {
      final Object? data = response['data'];
      return data is Map<Object?, Object?> ? data.cast<String, Object?>() : <String, Object?>{};
    }
    final Object? error = response['error'];
    if (error is Map<Object?, Object?>) {
      throw BridgeException('${error['code']}', '${error['message'] ?? ''}');
    }
    throw const BridgeException('internal', '');
  }

  Future<List<Session>> listSessions() async {
    final Json data = await request('sessions.list');
    return data.objects('sessions').map(Session.fromJson).toList();
  }

  Future<({List<ChatMessage> messages, bool hasMore, bool reset})> chatGet(String sessionId,
      {String? subagentId, int limit = 50, String? before, String? after}) async {
    final Json data = await request('chat.get', <String, Object?>{
      'sessionId': sessionId,
      'subagentId': ?subagentId,
      'limit': limit,
      'before': ?before,
      'after': ?after,
    });
    final List<ChatMessage> messages = data.objects('messages').map(ChatMessage.fromJson).toList();
    if (before == null && subagentId == null && messages.isNotEmpty) _newestChatId[sessionId] = messages.last.id;
    return (messages: messages, hasMore: data.boolean('hasMore'), reset: data['reset'] == true);
  }

  Future<Uint8List> chatImage(String sessionId, String imageId, {int maxWidth = 800}) async {
    final int id = _nextId++;
    final Completer<Uint8List> frame = Completer<Uint8List>();
    _imageWaiters[id] = frame;
    try {
      await request('chat.image', <String, Object?>{'sessionId': sessionId, 'imageId': imageId, 'maxWidth': maxWidth},
          const Duration(seconds: 30), id);
      return await frame.future.timeout(const Duration(seconds: 20));
    } on TimeoutException {
      throw const BridgeException('timeout', '');
    } finally {
      _imageWaiters.remove(id);
    }
  }

  Future<List<({String name, String description, String? argumentHint})>> commandsList(String sessionId) async {
    final Json data = await request('commands.list', <String, Object?>{'sessionId': sessionId});
    return <({String name, String description, String? argumentHint})>[
      for (final Json row in data.objects('commands')) (name: row.str('name'), description: row.strOrNull('description') ?? '', argumentHint: row.strOrNull('argumentHint')),
    ];
  }

  Future<void> sessionImage(String sessionId, Uint8List bytes, {String caption = '', void Function(double fraction)? onProgress}) async {
    final Json begin = await request(
      'session.image.begin',
      <String, Object?>{'sessionId': sessionId, 'bytes': bytes.length, if (caption.trim().isNotEmpty) 'caption': caption.trim()},
      const Duration(seconds: 30),
    );
    final String uploadId = begin.str('uploadId');
    final int piece = math.min(begin['chunkBytes'] is num ? (begin['chunkBytes']! as num).toInt() : 96 * 1024, 96 * 1024);
    int sequence = 0;
    for (int offset = 0; offset < bytes.length; offset += piece) {
      final int end = math.min(offset + piece, bytes.length);
      await request(
        'session.image.chunk',
        <String, Object?>{'uploadId': uploadId, 'seq': sequence++, 'data': base64Encode(Uint8List.sublistView(bytes, offset, end))},
        const Duration(seconds: 30),
      );
      onProgress?.call(end / bytes.length);
    }
    await request('session.image.end', <String, Object?>{'uploadId': uploadId}, const Duration(seconds: 60));
  }

  Future<Json> vizGet(String sessionId, {bool entity = false}) =>
      request('viz.get', <String, Object?>{'sessionId': sessionId, if (entity) 'entity': true}, const Duration(seconds: 30));

  Future<Uint8List> webSnapshot(String sessionId, {String? url, int width = 400}) async {
    final int id = _nextId++;
    final Completer<Uint8List> frame = Completer<Uint8List>();
    _snapshotWaiters[id] = frame;
    try {
      await request('web.snapshot', <String, Object?>{'sessionId': sessionId, 'width': width, 'url': ?url},
          const Duration(seconds: 30), id);
      return await frame.future.timeout(const Duration(seconds: 15));
    } on TimeoutException {
      throw const BridgeException('timeout', '');
    } finally {
      _snapshotWaiters.remove(id);
    }
  }

  Future<void> sessionMessage(String sessionId, String text) async {
    await request('session.message', <String, Object?>{'sessionId': sessionId, 'text': text}, const Duration(seconds: 60));
  }

  Future<void> sessionSendNow(String sessionId) async {
    await request('session.sendNow', <String, Object?>{'sessionId': sessionId}, const Duration(seconds: 15));
  }

  Future<void> promptAllow(String requestId, {required bool allow}) async {
    await request('prompt.answer', <String, Object?>{'requestId': requestId, 'allow': allow});
  }

  Future<void> promptChoose(String requestId, List<Object> choices) async {
    await request('prompt.answer', <String, Object?>{'requestId': requestId, 'choice': choices.length == 1 ? choices.single : choices});
  }

  Future<void> sessionNew(String cwd, {String? model}) async {
    await request('session.new', <String, Object?>{'cwd': cwd, 'model': ?model}, const Duration(seconds: 30));
  }

  Future<String> sessionTakeover(String sessionId, {bool stopOriginal = false}) async {
    final Json data = await request('session.takeover', <String, Object?>{'sessionId': sessionId, if (stopOriginal) 'stopOriginal': true},
        const Duration(seconds: 45));
    return data.str('sessionId');
  }

  Future<List<InboxItem>> inboxList() async {
    final Json data = await request('inbox.list');
    return data.objects('items').map(InboxItem.fromJson).toList();
  }

  Future<InboxAnswer> inboxAnswer(String itemId, String action) async {
    final Json data = await request('inbox.answer', <String, Object?>{'itemId': itemId, 'action': action});
    return InboxAnswer(answered: data.boolean('answered'), state: data.strOrNull('state') ?? 'answered', answer: data.strOrNull('answer'));
  }

  Future<List<String>> projectsRecent() async => (await request('projects.recent')).strings('projects');

  Future<void> ping() async {
    await request('ping');
  }

  Future<void> subscribe(SubscriptionSpec spec) async {
    _subscription = spec;
    if (!isConnected) return;
    await request('subscribe', _subscribeParams(spec));
  }

  Future<void> unsubscribe() async {
    _subscription = null;
    if (!isConnected) return;
    await request('unsubscribe');
  }
}

Future<({String token, String host, bool viaTunnel})> pairWithMac({
  required WsConnector connector,
  required List<String> hosts,
  required int port,
  required String fingerprint,
  required String code,
  required String deviceName,
  required String platform,
  TunnelInfo? tunnel,
  Duration approvalTimeout = const Duration(seconds: 120),
}) async {
  Object? lastError;
  final List<Endpoint> routes = <Endpoint>[
    for (final String host in hosts) Endpoint(host: host, port: port, fingerprint: fingerprint, timeout: tunnel == null ? const Duration(seconds: 6) : const Duration(seconds: 3)),
    if (tunnel != null) Endpoint.viaTunnel(tunnel),
  ];
  for (final Endpoint route in routes) {
    final String host = route.host;
    WsConnection? connection;
    try {
      connection = await connector(route);
      final Completer<Json> answer = Completer<Json>();
      connection.messages.listen((Object message) {
        if (message is! String || answer.isCompleted) return;
        final Object? decoded = jsonDecode(message);
        if (decoded is Map<String, Object?>) answer.complete(decoded);
      }, onDone: () {
        if (!answer.isCompleted) answer.completeError(const BridgeException('disconnected', ''));
      });
      connection.sendText(jsonEncode(<String, Object?>{
        'id': 1,
        'op': 'pair',
        'code': code,
        'device': <String, Object?>{'name': deviceName, 'platform': platform},
      }));
      final Json response = await answer.future.timeout(approvalTimeout);
      if (response['ok'] == true) {
        return (token: response.object('data').str('token'), host: host, viaTunnel: route.kind == RouteKind.tunnel);
      }
      final Object? error = response['error'];
      throw BridgeException(error is Map<Object?, Object?> ? '${error['code']}' : 'internal', error is Map<Object?, Object?> ? '${error['message'] ?? ''}' : '');
    } on FingerprintMismatch {
      rethrow;
    } on BridgeException {
      rethrow;
    } on Object catch (error) {
      lastError = error;
    } finally {
      await connection?.close();
    }
  }
  throw lastError ?? const BridgeException('disconnected', '');
}
