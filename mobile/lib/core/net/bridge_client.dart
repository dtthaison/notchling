import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import '../pairing/mac_profile.dart';
import '../protocol/json.dart';
import '../protocol/models.dart';
import 'tunnel.dart';
import 'ws_transport.dart';

/// An error answer from the Mac ({"ok":false,"error":{"code","message"}}), or a local failure
/// (`timeout`, `disconnected`).
class BridgeException implements Exception {
  const BridgeException(this.code, this.message);
  final String code;
  final String message;

  bool get isViewOnly => code == 'view_only';
  bool get isUnauthorized => code == 'unauthorized';
  bool get isNotFound => code == 'not_found';

  @override
  String toString() => 'BridgeException($code): $message';
}

enum LinkPhase { connecting, connected, disconnected }

class ConnectionStatus {
  const ConnectionStatus(this.phase, {this.retryAt, this.error, this.host, this.route, this.revoked = false});
  final LinkPhase phase;

  /// How the link reaches the Mac while connected: home network or the internet tunnel.
  final RouteKind? route;

  /// When the next attempt starts (while disconnected).
  final DateTime? retryAt;
  final Object? error;
  final String? host;

  /// The Mac no longer accepts our token (it was revoked): reconnecting is pointless.
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

/// A prompt appeared ([prompt] set) or went away ([resolvedRequestId] set).
class PromptEvent extends BridgeEvent {
  const PromptEvent({this.prompt, this.resolvedRequestId});
  final PendingPrompt? prompt;
  final String? resolvedRequestId;
}

class OpenEvent extends BridgeEvent {
  const OpenEvent();
}

/// A frame of the phone stream: 4-byte magic "PHN1", uint32 big-endian sequence, then the JPEG.
class PhoneFrame {
  const PhoneFrame(this.sequence, this.jpeg);
  final int sequence;
  final Uint8List jpeg;
}

/// What this connection wants pushed. Re-sent after every reconnect.
class SubscriptionSpec {
  const SubscriptionSpec({this.sessions = false, this.sessionId, this.subagentId, this.phoneFps, this.phoneWidth, this.phoneWindowId});
  final bool sessions;
  final String? sessionId;
  final String? subagentId;
  final int? phoneFps;
  final int? phoneWidth;
  final int? phoneWindowId;

  bool get wantsPhone => phoneFps != null;

  Json toParams() => <String, Object?>{
        if (sessions) 'sessions': true,
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

/// The typed client for the Mac bridge protocol (Console/Sources/ConsoleCore/Bridge). It keeps the link
/// up on its own: it tries the Mac's hosts in order, says hello, re-subscribes, and backs off between rounds.
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

  /// Called with the route that just worked, so the caller can remember it.
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

  /// The newest chat message seen per session. Sent with every subscribe so the Mac pushes what came after it.
  final Map<String, String> _newestChatId = <String, String>{};

  /// Notes that the phone already has [messageId] of [sessionId] (for instance from its disk cache).
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
    _failPending(const BridgeException('disconnected', 'Đã ngắt kết nối.'));
    await connection?.close();
    _setStatus(const ConnectionStatus(LinkPhase.disconnected));
  }

  Future<void> dispose() async {
    await stop();
    await _statusController.close();
    await _eventController.close();
    await _frameController.close();
  }

  /// Skips the wait before the next attempt.
  void retryNow() => _wake?.complete();

  /// The phone's network changed (Wi-Fi to mobile data or back). Routes are tried again with the LAN first,
  /// so the link moves to the home network when the phone arrives and to the tunnel when it leaves.
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

  // MARK: - Connection loop

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
        _failPending(const BridgeException('disconnected', 'Mất kết nối.'));
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
        } on Object {
          // Already gone.
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
    } on BridgeException {
      // The caller sees the same error when it subscribes itself; a failed phone stream must not drop the link.
    }
  }

  // MARK: - Wire

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
        case 'open':
          _eventController.add(const OpenEvent());
      }
    } on FormatException {
      // A push we cannot read is dropped; the next full list corrects any drift.
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

  /// Sends one request and returns its `data`. Errors from the Mac become [BridgeException].
  Future<Json> request(String op, [Json params = const <String, Object?>{}, Duration? timeout, int? idOverride]) async {
    final WsConnection? connection = _connection;
    if (connection == null) throw const BridgeException('disconnected', 'Chưa kết nối tới Mac.');
    final int id = idOverride ?? _nextId++;
    final Completer<Json> completer = Completer<Json>();
    _pending[id] = completer;
    connection.sendText(jsonEncode(<String, Object?>{'id': id, 'op': op, ...params}));
    final Json response;
    try {
      response = await completer.future.timeout(timeout ?? requestTimeout);
    } on TimeoutException {
      _pending.remove(id);
      throw BridgeException('timeout', 'Mac không trả lời $op.');
    }
    if (response['ok'] == true) {
      final Object? data = response['data'];
      return data is Map<Object?, Object?> ? data.cast<String, Object?>() : <String, Object?>{};
    }
    final Object? error = response['error'];
    if (error is Map<Object?, Object?>) {
      throw BridgeException('${error['code']}', '${error['message']}');
    }
    throw const BridgeException('internal', 'Phản hồi không đúng định dạng.');
  }

  // MARK: - Ops

  Future<List<Session>> listSessions() async {
    final Json data = await request('sessions.list');
    return data.objects('sessions').map(Session.fromJson).toList();
  }

  /// Messages oldest to newest. `before` is the id of the oldest message you already have; `after` the newest.
  /// With `after`, [reset] true means the Mac no longer has that message in reach and the page is the newest
  /// window to show instead of what you have.
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

  /// A JPEG of a picture in [sessionId]'s chat, at most [maxWidth] wide.
  Future<Uint8List> chatImage(String sessionId, String imageId, {int maxWidth = 800}) async {
    final int id = _nextId++;
    final Completer<Uint8List> frame = Completer<Uint8List>();
    _imageWaiters[id] = frame;
    try {
      await request('chat.image', <String, Object?>{'sessionId': sessionId, 'imageId': imageId, 'maxWidth': maxWidth},
          const Duration(seconds: 30), id);
      return await frame.future.timeout(const Duration(seconds: 20));
    } on TimeoutException {
      throw const BridgeException('timeout', 'Mac không gửi ảnh.');
    } finally {
      _imageWaiters.remove(id);
    }
  }

  Future<Json> vizGet(String sessionId, {bool entity = false}) =>
      request('viz.get', <String, Object?>{'sessionId': sessionId, if (entity) 'entity': true}, const Duration(seconds: 30));

  /// A JPEG of a localhost page (the session's own URL when [url] is null).
  Future<Uint8List> webSnapshot(String sessionId, {String? url, int width = 400}) async {
    final int id = _nextId++;
    final Completer<Uint8List> frame = Completer<Uint8List>();
    _snapshotWaiters[id] = frame;
    try {
      await request('web.snapshot', <String, Object?>{'sessionId': sessionId, 'width': width, 'url': ?url},
          const Duration(seconds: 30), id);
      return await frame.future.timeout(const Duration(seconds: 15));
    } on TimeoutException {
      throw const BridgeException('timeout', 'Mac không gửi ảnh trang.');
    } finally {
      _snapshotWaiters.remove(id);
    }
  }

  /// Sends [text] to exactly [sessionId]. A view-only session answers `view_only`.
  Future<void> sessionMessage(String sessionId, String text) async {
    await request('session.message', <String, Object?>{'sessionId': sessionId, 'text': text}, const Duration(seconds: 60));
  }

  Future<void> promptAllow(String requestId, {required bool allow}) async {
    await request('prompt.answer', <String, Object?>{'requestId': requestId, 'allow': allow});
  }

  /// One entry per question: a 1-based option number or an option label.
  Future<void> promptChoose(String requestId, List<Object> choices) async {
    await request('prompt.answer', <String, Object?>{'requestId': requestId, 'choice': choices.length == 1 ? choices.single : choices});
  }

  Future<void> sessionNew(String cwd, {String? model}) async {
    await request('session.new', <String, Object?>{'cwd': cwd, 'model': ?model}, const Duration(seconds: 30));
  }

  /// Resumes a view-only session whose original process is gone in a Console-owned terminal on the Mac.
  /// Returns the id of the session to switch to (it may still be a starting terminal's `pty:` id).
  Future<String> sessionTakeover(String sessionId, {bool stopOriginal = false}) async {
    final Json data = await request('session.takeover', <String, Object?>{'sessionId': sessionId, if (stopOriginal) 'stopOriginal': true},
        const Duration(seconds: 45));
    return data.str('sessionId');
  }

  Future<List<String>> projectsRecent() async => (await request('projects.recent')).strings('projects');

  Future<void> ping() async {
    await request('ping');
  }

  /// Replaces what the Mac pushes to this connection and remembers it for reconnects.
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

/// Pairs with a Mac: tries each host with the one-time code, then the tunnel if the QR had one, and returns
/// the device token. Throws [FingerprintMismatch] if a host presents another certificate, [BridgeException]
/// if the Mac says no.
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
        if (!answer.isCompleted) answer.completeError(const BridgeException('disconnected', 'Mac đóng kết nối.'));
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
      throw BridgeException(error is Map<Object?, Object?> ? '${error['code']}' : 'internal', error is Map<Object?, Object?> ? '${error['message']}' : '');
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
  throw lastError ?? const BridgeException('disconnected', 'Không có địa chỉ nào để thử.');
}
