import '../../core/protocol/models.dart';

class SessionRef {
  const SessionRef(this.host, this.id);

  final String host;

  final String id;

  String get qualified => host.isEmpty ? id : '$host/$id';

  static final RegExp _pattern = RegExp(r'^([0-9a-f]{16})/(.+)$', dotAll: true);

  static SessionRef? parse(String qualified) {
    final RegExpMatch? match = _pattern.firstMatch(qualified);
    return match == null ? null : SessionRef(match.group(1)!, match.group(2)!);
  }

  static SessionRef fromPayload(String payload, {required String fallbackHost}) => parse(payload) ?? SessionRef(fallbackHost, payload);

  SessionRef onHostIfUnknown(String defaultHost) => host.isEmpty ? SessionRef(defaultHost, id) : this;

  @override
  bool operator ==(Object other) => other is SessionRef && other.host == host && other.id == id;

  @override
  int get hashCode => Object.hash(host, id);

  @override
  String toString() => 'SessionRef($qualified)';
}

const String bareMark = '/';

String storageKey(String scope, String sessionId) {
  if (scope.isNotEmpty) return '$scope/$sessionId';
  return SessionRef.parse(sessionId) == null ? sessionId : '$bareMark$sessionId';
}

bool keyInScope(String scope, String key) =>
    scope.isEmpty ? key.startsWith(bareMark) || SessionRef.parse(key) == null : key.startsWith('$scope/');

String? idInBareScope(String key) {
  if (key.startsWith(bareMark)) return key.substring(bareMark.length);
  return SessionRef.parse(key) == null ? key : null;
}

Session withSessionId(Session session, String id) => Session(
      id: id,
      name: session.name,
      project: session.project,
      cwd: session.cwd,
      entrypoint: session.entrypoint,
      state: session.state,
      control: session.control,
      lastActivity: session.lastActivity,
      subagents: session.subagents,
      pending: session.pending,
      takeover: session.takeover,
      activity: session.activity,
      facts: session.facts,
      lastMessage: session.lastMessage,
      node: session.node,
    );
