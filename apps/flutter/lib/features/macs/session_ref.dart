import '../../core/protocol/models.dart';

/// A session on one computer. Two computers can have a session with the same id (`pty:3`, or a copied
/// transcript), so the phone never keeps a bare id where it could act on it: it keeps the computer too.
/// Decision 0012 defines the format.
class SessionRef {
  const SessionRef(this.host, this.id);

  /// The computer's id: the first 16 lowercase hex characters of its certificate's SHA-256 fingerprint
  /// (`MacProfile.id`). Empty only before any computer is paired.
  final String host;

  /// The session id exactly as that computer gave it.
  final String id;

  /// `<host>/<session id>`. Split at the first slash: the host never has one, a session id may.
  String get qualified => host.isEmpty ? id : '$host/$id';

  static final RegExp _pattern = RegExp(r'^([0-9a-f]{16})/(.+)$', dotAll: true);

  /// The ref in a qualified id, or null for a bare one. No id a computer gives starts with 16 hex
  /// characters and a slash (Claude Code's are UUIDs, the others `pty:`, `codex:`, `agy:`).
  static SessionRef? parse(String qualified) {
    final RegExpMatch? match = _pattern.firstMatch(qualified);
    return match == null ? null : SessionRef(match.group(1)!, match.group(2)!);
  }

  /// The session a notification payload names: a qualified id, or a bare one, which an older notification carries
  /// and which means the computer in use, [fallbackHost].
  static SessionRef fromPayload(String payload, {required String fallbackHost}) => parse(payload) ?? SessionRef(fallbackHost, payload);

  /// This ref, on [defaultHost] when it names no computer.
  SessionRef onHostIfUnknown(String defaultHost) => host.isEmpty ? SessionRef(defaultHost, id) : this;

  @override
  bool operator ==(Object other) => other is SessionRef && other.host == host && other.id == id;

  @override
  int get hashCode => Object.hash(host, id);

  @override
  String toString() => 'SessionRef($qualified)';
}

/// The key a phone-side store (chat cache, archive) uses for a session: the bare id under the empty scope, which
/// is what a phone paired before 0012 used for its Mac, otherwise `<scope>/<id>`.
String storageKey(String scope, String sessionId) => scope.isEmpty ? sessionId : '$scope/$sessionId';

/// Whether a phone-side storage [key] (see [storageKey]) belongs to the computer whose storage scope is [scope]:
/// `<scope>/…` for a scope, and for the empty scope every key that carries no host (a bare id).
bool keyInScope(String scope, String key) => scope.isEmpty ? SessionRef.parse(key) == null : key.startsWith('$scope/');

/// [session] with its id replaced, for lists that hold sessions of several computers.
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
    );
