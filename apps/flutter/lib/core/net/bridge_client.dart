import 'dart:async';
import 'dart:math' as math;
import 'dart:convert';
import 'dart:typed_data';

import '../pairing/mac_profile.dart';
import '../protocol/generated.dart' as wire;
import '../protocol/json.dart';
import '../protocol/models.dart';
import 'command_key.dart';
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
  const ConnectionStatus(
    this.phase, {
    this.retryAt,
    this.error,
    this.host,
    this.route,
    this.revoked = false,
    this.via,
    this.primaryName,
    this.primaryAnswers = false,
    this.refused = const <String>[],
  });
  final LinkPhase phase;

  final String? via;
  final String? primaryName;
  final bool primaryAnswers;
  final List<String> refused;

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

class HeldEvent extends BridgeEvent {
  const HeldEvent(this.entries, this.notices);
  final List<wire.HeldEntry> entries;
  final List<wire.HeldNotice> notices;
}

class HeldHintEvent extends BridgeEvent {
  const HeldHintEvent();
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
  const SubscriptionSpec({this.sessions = false, this.inbox = false, this.held = false, this.sessionId, this.subagentId, this.phoneFps, this.phoneWidth, this.phoneWindowId});
  final bool sessions;
  final bool inbox;
  final bool held;
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
        if (held) 'held': true,
        if (wantsPhone)
          'phone': <String, Object?>{'fps': phoneFps, 'width': phoneWidth ?? 400, 'deviceWindowId': ?phoneWindowId},
      };
}

const Duration holderFresh = Duration(hours: 1, minutes: 5);

const List<Duration> defaultBackoff = <Duration>[
  Duration(seconds: 1),
  Duration(seconds: 2),
  Duration(seconds: 4),
  Duration(seconds: 8),
  Duration(seconds: 16),
  Duration(seconds: 30),
];

class _Route {
  const _Route(this.endpoint, this.node);
  final Endpoint endpoint;
  final PinnedNode? node;
}

class _Link {
  const _Link(this.connection, this.closed, this.route, this.node, this.hello);
  final WsConnection connection;
  final Completer<void> closed;
  final Endpoint route;
  final PinnedNode? node;
  final Json hello;
}

({String host, int port})? splitAddress(String address) {
  final String text = address.trim();
  final String host;
  final String portText;
  if (text.startsWith('[')) {
    final int close = text.indexOf(']');
    if (close < 2 || text.length < close + 3 || text[close + 1] != ':') return null;
    host = text.substring(1, close);
    portText = text.substring(close + 2);
  } else {
    final int colon = text.lastIndexOf(':');
    if (colon <= 0 || text.indexOf(':') != colon) return null;
    host = text.substring(0, colon);
    portText = text.substring(colon + 1);
  }
  final int? port = int.tryParse(portText);
  if (host.isEmpty || port == null || port < 1 || port > 65535) return null;
  return (host: host, port: port);
}

class BridgeClient {
  BridgeClient({
    required this.connector,
    required this.profile,
    this.requestTimeout = const Duration(seconds: 15),
    this.backoff = defaultBackoff,
    this.onGoodRoute,
    this.onCluster,
    this.primaryRetry = const Duration(seconds: 15),
    DateTime Function()? clock,
  }) : _clock = clock ?? DateTime.now;

  final WsConnector connector;
  final MacProfile profile;
  final Duration requestTimeout;
  final List<Duration> backoff;

  final void Function(Endpoint route)? onGoodRoute;
  final void Function(String? primary, int term, List<PinnedNode> nodes)? onCluster;
  final Duration primaryRetry;
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
  late List<PinnedNode> _nodes = List<PinnedNode>.of(profile.nodes);
  late String? _clusterPrimary = profile.clusterPrimary;
  bool _sawCluster = false;
  late int _term = profile.clusterTerm;
  final Map<String, DateTime> _holders = <String, DateTime>{};
  String? _claimedPrimary;
  int _claimedTerm = 0;
  PinnedNode? _via;
  List<String> _refused = <String>[];
  bool _unauthorized = false;
  String? _avoid;
  DateTime? _lastRotate;
  ConnectionStatus _status = const ConnectionStatus(LinkPhase.disconnected);

  Stream<ConnectionStatus> get status => _statusController.stream;
  Stream<BridgeEvent> get events => _eventController.stream;
  Stream<PhoneFrame> get phoneFrames => _frameController.stream;
  ConnectionStatus get currentStatus => _status;
  bool get isConnected => _status.phase == LinkPhase.connected;
  bool get holding => _holders.isNotEmpty;

  String? get _primaryId => _claimedPrimary ?? _clusterPrimary;

  void markHeld(bool entries) {
    final String key = _via?.id ?? '';
    if (entries) {
      _holders[key] = _clock();
    } else {
      _holders.remove(key);
    }
  }

  bool get inCluster => _sawCluster || _nodes.isNotEmpty;
  String get nodeName => _via?.name ?? profile.name;
  String get holderKey => _via?.id ?? '';

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
      final _Link? link = await _openAny((Object error) => lastError = error);
      if (_stopped) {
        await link?.connection.close();
        break;
      }
      if (link != null) {
        round = 0;
        _install(link);
        _learn(link);
        _setStatus(_connectedStatus(link));
        unawaited(_resubscribe());
        await _stayConnected(link);
        _connection = null;
        _failPending(const BridgeException('disconnected', ''));
        if (_stopped) break;
        if (_reconnectNow) {
          _reconnectNow = false;
          continue;
        }
      } else if (_unauthorized) {
        _setStatus(ConnectionStatus(LinkPhase.disconnected, error: lastError, revoked: true));
        return;
      }
      final Duration delay = backoff[round < backoff.length ? round : backoff.length - 1];
      round++;
      _setStatus(ConnectionStatus(LinkPhase.disconnected, retryAt: _clock().add(delay), error: lastError, refused: _refused));
      _wake = Completer<void>();
      await Future.any<void>(<Future<void>>[Future<void>.delayed(delay), _wake!.future]);
      _wake = null;
    }
  }

  void _install(_Link link) {
    _connection = link.connection;
    _via = link.node;
    _preferLan = false;
    final String holderKey = link.node?.id ?? '';
    if (_holders.containsKey(holderKey)) _holders[holderKey] = _clock();
    if (link.node == null) onGoodRoute?.call(link.route);
  }

  ConnectionStatus _connectedStatus(_Link link, {bool primaryAnswers = false}) {
    final PinnedNode? node = link.node;
    return ConnectionStatus(
      LinkPhase.connected,
      host: link.route.host,
      route: link.route.kind,
      via: node?.name,
      primaryName: node == null || node.id == _primaryId ? null : _primaryName(),
      primaryAnswers: primaryAnswers,
    );
  }

  String _primaryName() {
    for (final PinnedNode node in _nodes) {
      if (node.id == _primaryId) return node.name;
    }
    return profile.name;
  }

  Future<void> _stayConnected(_Link first) async {
    _Link link = first;
    while (!_stopped) {
      if (link.node == null) {
        await link.closed.future;
        return;
      }
      final Completer<void> tick = Completer<void>();
      final Timer timer = Timer(primaryRetry, () {
        if (!tick.isCompleted) tick.complete();
      });
      await Future.any<void>(<Future<void>>[link.closed.future, tick.future]);
      timer.cancel();
      if (link.closed.isCompleted || _stopped) return;
      final Future<_Link?> probe = _probePrimary();
      final _Link? back = await Future.any<_Link?>(<Future<_Link?>>[probe, link.closed.future.then((_) => null)]);
      if (_stopped || link.closed.isCompleted) {
        unawaited(probe.then((_Link? found) => found?.connection.close()));
        return;
      }
      if (back == null) {
        _markPrimaryAnswers(link, false);
        continue;
      }
      if (_holders.containsKey(link.node?.id ?? '')) {
        await back.connection.close();
        _markPrimaryAnswers(link, true);
        continue;
      }
      _failPending(const BridgeException('disconnected', ''));
      final _Link old = link;
      link = back;
      _install(back);
      _learn(back);
      _setStatus(_connectedStatus(back));
      unawaited(_resubscribe());
      await old.connection.close();
    }
  }

  void _markPrimaryAnswers(_Link link, bool answers) {
    if (_status.phase != LinkPhase.connected || _status.primaryAnswers == answers) return;
    _setStatus(_connectedStatus(link, primaryAnswers: answers));
  }

  Future<_Link?> _probePrimary() async {
    for (final Endpoint endpoint in profile.endpoints(preferLan: true)) {
      if (_stopped) return null;
      final _Link? link = await _openRoute(_Route(endpoint, null), (Object _) {}, probe: true);
      if (link != null) return link;
    }
    return null;
  }

  Future<_Link?> _openAny(void Function(Object error) onError) async {
    _unauthorized = false;
    final List<_Route> routes = _routes();
    for (final _Route route in routes) {
      if (_stopped) return null;
      final _Link? link = await _openRoute(route, onError);
      if (link != null) return link;
      if (_unauthorized) return null;
    }
    return null;
  }

  List<_Route> _routes() {
    final List<_Route> primary = <_Route>[for (final Endpoint e in profile.endpoints(preferLan: _preferLan)) _Route(e, null)];
    final List<_Route> others = _fallbackRoutes();
    final String? avoid = _avoid;
    _avoid = null;
    if (avoid == null) {
      final List<_Route> all = <_Route>[...primary, ...others];
      final DateTime now = _clock();
      final Iterable<String> holders = _holders.entries
          .where((MapEntry<String, DateTime> e) => now.difference(e.value) <= holderFresh)
          .map((MapEntry<String, DateTime> e) => e.key);
      if (holders.isEmpty) return all;
      final String holder = holders.first;
      return <_Route>[
        ...all.where((_Route r) => (r.node?.id ?? '') == holder),
        ...all.where((_Route r) => (r.node?.id ?? '') != holder),
      ];
    }
    if (avoid.isEmpty) return <_Route>[...others, ...primary];
    return <_Route>[
      ...primary,
      ...others.where((_Route r) => r.node?.id != avoid),
      ...others.where((_Route r) => r.node?.id == avoid),
    ];
  }

  List<_Route> _fallbackRoutes() {
    final Set<String> own = <String>{for (final String host in profile.hosts) '$host:${profile.port}'};
    final List<PinnedNode> ordered = <PinnedNode>[
      ..._nodes.where((PinnedNode n) => n.id == _primaryId),
      ..._nodes.where((PinnedNode n) => n.id != _primaryId && n.online),
      ..._nodes.where((PinnedNode n) => n.id != _primaryId && !n.online),
    ];
    final List<String> refused = <String>[];
    final List<_Route> routes = <_Route>[];
    for (final PinnedNode node in ordered) {
      if (!node.canBeMaster) continue;
      if (!node.hasValidPin) {
        if (!refused.contains(node.name)) refused.add(node.name);
        continue;
      }
      for (final String addr in node.addrs) {
        final ({String host, int port})? at = splitAddress(addr);
        if (at == null || own.contains('${at.host}:${at.port}')) continue;
        routes.add(_Route(
          Endpoint(host: at.host, port: at.port, fingerprint: node.fingerprint.trim().toLowerCase(), timeout: const Duration(seconds: 4)),
          node,
        ));
      }
    }
    _refused = refused;
    return routes;
  }

  Future<_Link?> _openRoute(_Route route, void Function(Object error) onError, {bool probe = false}) async {
    WsConnection? connection;
    try {
      connection = await connector(route.endpoint);
      final Completer<void> closed = Completer<void>();
      connection.messages.listen(
        _onMessage,
        onDone: () { if (!closed.isCompleted) closed.complete(); },
        onError: (Object _) { if (!closed.isCompleted) closed.complete(); },
        cancelOnError: true,
      );
      final Json hello = await _requestOn(connection, 'hello', <String, Object?>{'token': profile.token}, const Duration(seconds: 8));
      return _Link(connection, closed, route.endpoint, route.node, hello);
    } on Object catch (error) {
      onError(error);
      try {
        await connection?.close();
      } on Object catch (_) {
      }
      final PinnedNode? node = route.node;
      if (error is FingerprintMismatch && node != null && !_refused.contains(node.name)) _refused = <String>[..._refused, node.name];
      if (error is BridgeException && error.isUnauthorized && node == null && !probe) _unauthorized = true;
      return null;
    }
  }

  void _learn(_Link link) {
    final bool paired = link.node == null && link.route.kind == RouteKind.lan;
    final Object? raw = link.hello['cluster'];
    if (raw is! Map<Object?, Object?>) {
      if (!paired) return;
      final bool had = _inCluster();
      _sawCluster = false;
      _nodes = <PinnedNode>[];
      _clusterPrimary = null;
      _claimedPrimary = null;
      _claimedTerm = 0;
      _term = 0;
      if (had) onCluster?.call(null, 0, const <PinnedNode>[]);
      return;
    }
    final wire.HelloCluster cluster;
    try {
      cluster = wire.HelloCluster.fromJson(raw.cast<String, Object?>());
    } on FormatException catch (_) {
      return;
    }
    _sawCluster = true;
    if (paired) {
      if (cluster.term < _term) return;
      final List<PinnedNode> next = <PinnedNode>[
        for (final wire.ClusterNode n in cluster.nodes)
          PinnedNode(id: n.id, name: n.name, addrs: n.addrs, fingerprint: n.fingerprint, canBeMaster: n.canBeMaster, online: n.online),
      ];
      final bool same = next.length == _nodes.length &&
          _clusterPrimary == cluster.primary &&
          _term == cluster.term &&
          <bool>[for (int i = 0; i < next.length; i++) jsonEncode(next[i].toJson()) == jsonEncode(_nodes[i].toJson())].every((bool b) => b);
      _nodes = next;
      _clusterPrimary = cluster.primary;
      _claimedPrimary = null;
      _claimedTerm = 0;
      _term = cluster.term;
      if (!same) onCluster?.call(cluster.primary, cluster.term, next);
      return;
    }
    if (link.node != null && cluster.term > _term && cluster.term > _claimedTerm && _nodes.any((PinnedNode n) => n.id == cluster.primary)) {
      _claimedPrimary = cluster.primary;
      _claimedTerm = cluster.term;
    }
  }

  bool _inCluster() => _sawCluster || _nodes.isNotEmpty;

  void _onPrimaryUnreachable(WsConnection from) {
    if (_stopped || _connection != from) return;
    if (_holders.containsKey(_via?.id ?? '')) return;
    if (_via == null && _fallbackRoutes().isEmpty) return;
    final DateTime now = _clock();
    final DateTime? last = _lastRotate;
    if (last != null && now.difference(last) < primaryRetry) return;
    _lastRotate = now;
    _avoid = _via?.id ?? '';
    _reconnectNow = true;
    unawaited(from.close());
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
        case 'held.changed':
          _eventController.add(HeldEvent(_heldEntries(json), _heldNotices(json)));
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

  Future<Json> request(String op, [Json params = const <String, Object?>{}, Duration? timeout, int? idOverride]) {
    final WsConnection? connection = _connection;
    if (connection == null) throw const BridgeException('disconnected', '');
    return _requestOn(connection, op, params, timeout, idOverride);
  }

  Future<Json> _requestOn(WsConnection connection, String op, Json params, [Duration? timeout, int? idOverride]) async {
    final int id = idOverride ?? _nextId++;
    final Completer<Json> completer = Completer<Json>();
    _pending[id] = completer;
    try {
      connection.sendText(jsonEncode(<String, Object?>{'id': id, 'op': op, ...params}));
    } on Object catch (_) {
      _pending.remove(id);
      throw const BridgeException('disconnected', '');
    }
    final Json response;
    try {
      response = await completer.future.timeout(timeout ?? requestTimeout);
    } on TimeoutException {
      _pending.remove(id);
      throw const BridgeException('timeout', '');
    }
    if (response['ok'] == true) {
      final Object? data = response['data'];
      final Json result = data is Map<Object?, Object?> ? data.cast<String, Object?>() : <String, Object?>{};
      if (result['held'] is Map<Object?, Object?> && !_eventController.isClosed) _eventController.add(const HeldHintEvent());
      return result;
    }
    final Object? error = response['error'];
    if (error is Map<Object?, Object?>) {
      if (error['code'] == 'primary_unreachable') _onPrimaryUnreachable(connection);
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

  Future<void> sessionMessage(String sessionId, String text, {String? key}) async {
    await request('session.message', <String, Object?>{'sessionId': sessionId, 'text': text, 'key': key ?? newCommandKey()}, const Duration(seconds: 60));
  }

  Future<void> sessionSendNow(String sessionId, {String? key}) async {
    await request('session.sendNow', <String, Object?>{'sessionId': sessionId, 'key': key ?? newCommandKey()}, const Duration(seconds: 15));
  }

  Future<String?> promptAllow(String requestId, {required bool allow, String? key}) async {
    final Json data = await request('prompt.answer', <String, Object?>{'requestId': requestId, 'allow': allow, 'key': key ?? newCommandKey()});
    return _heldIdOf(data);
  }

  Future<String?> promptChoose(String requestId, List<Object> choices, {String? key}) async {
    final Json data = await request(
      'prompt.answer',
      <String, Object?>{'requestId': requestId, 'choice': choices.length == 1 ? choices.single : choices, 'key': key ?? newCommandKey()},
    );
    return _heldIdOf(data);
  }

  Future<void> sessionNew(String cwd, {String? model, String? key}) async {
    await request('session.new', <String, Object?>{'cwd': cwd, 'model': ?model, 'key': key ?? newCommandKey()}, const Duration(seconds: 30));
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

  Future<({List<wire.HeldEntry> entries, List<wire.HeldNotice> notices})> heldList() async {
    final Json data = await request('held.list');
    return (entries: _heldEntries(data), notices: _heldNotices(data));
  }

  Future<({bool done, bool delivered, bool inFlight})> heldCancel({String? heldId, String? key}) async {
    final Json data = await request('held.cancel', <String, Object?>{'heldId': ?heldId, 'key': ?key});
    return (done: data.boolean('cancelled'), delivered: data.boolean('delivered'), inFlight: data.boolean('inFlight'));
  }

  Future<bool> heldConfirm(String heldId) async {
    final Json data = await request('held.confirm', <String, Object?>{'heldId': heldId});
    return data.boolean('confirmed');
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

String? _heldIdOf(Json data) {
  final Object? held = data['held'];
  if (held is! Map<Object?, Object?>) return null;
  final Object? id = held['id'];
  return id is String ? id : null;
}

List<wire.HeldEntry> _heldEntries(Json json) {
  final List<wire.HeldEntry> entries = <wire.HeldEntry>[];
  for (final Json item in _lenient(json['entries'])) {
    try {
      entries.add(wire.HeldEntry.fromJson(item));
    } on FormatException catch (_) {
    }
  }
  return entries;
}

List<wire.HeldNotice> _heldNotices(Json json) {
  final List<wire.HeldNotice> notices = <wire.HeldNotice>[];
  for (final Json item in _lenient(json['notices'])) {
    try {
      notices.add(wire.HeldNotice.fromJson(item));
    } on FormatException catch (_) {
    }
  }
  return notices;
}

Iterable<Json> _lenient(Object? raw) sync* {
  if (raw is! List<Object?>) return;
  for (final Object? item in raw) {
    if (item is Map<Object?, Object?>) yield item.cast<String, Object?>();
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
