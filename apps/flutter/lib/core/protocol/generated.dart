// GENERATED CODE - DO NOT MODIFY BY HAND.
// The agentconsole.v1 wire types, from protocol/agentconsole.v1.schema.json by tools/protocol/gen.py.
// Change the schema and run the generator. The app still reads the wire with models.dart.
// ignore_for_file: unused_element

/// A JSON object.
typedef JsonMap = Map<String, Object?>;

Never _fail(String want, Object? got) => throw FormatException('expected $want', got);

String _asString(Object? v) => v is String ? v : _fail('a string', v);

int _asInt(Object? v) => v is int ? v : (v is num && v == v.truncate() ? v.toInt() : _fail('an integer', v));

double _asDouble(Object? v) => v is num ? v.toDouble() : _fail('a number', v);

bool _asBool(Object? v) => v is bool ? v : _fail('a boolean', v);

Object? _asAny(Object? v) => v;

JsonMap _asMap(Object? v) => v is Map<Object?, Object?> ? v.cast<String, Object?>() : _fail('an object', v);

List<T> _asList<T>(Object? v, T Function(Object?) read) =>
    v is List<Object?> ? <T>[for (final Object? item in v) read(item)] : _fail('a list', v);

T _read<T>(JsonMap json, String key, T Function(Object?) read) {
  try {
    return read(json[key]);
  } on FormatException catch (error) {
    throw FormatException('"$key": ${error.message}', error.source);
  }
}

T _need<T>(JsonMap json, String key, T Function(Object?) read) {
  if (!json.containsKey(key)) throw FormatException('missing "$key"', json);
  return _read(json, key, read);
}

T? _maybe<T>(JsonMap json, String key, T Function(Object?) read) {
  if (json[key] == null) return null;
  return _read(json, key, read);
}

/// Why the Mac refused a request.
enum ErrorCode {
  unauthorized('unauthorized'),
  notFound('not_found'),
  viewOnly('view_only'),
  invalid('invalid'),
  busy('busy'),
  notReachable('not_reachable'),
  internal('internal'),
  unknown('unknown');

  const ErrorCode(this.wire);

  /// The value on the wire.
  final String wire;

  /// The case for a wire value; [unknown] for one this version does not know.
  static ErrorCode parse(String raw) {
    for (final ErrorCode value in ErrorCode.values) {
      if (value.wire == raw) return value;
    }
    return ErrorCode.unknown;
  }
}

/// What the session is doing.
enum SessionState {
  needsInput('needsInput'),
  running('running'),
  done('done'),
  exited('exited'),
  unknown('unknown');

  const SessionState(this.wire);

  /// The value on the wire.
  final String wire;

  /// The case for a wire value; [unknown] for one this version does not know.
  static SessionState parse(String raw) {
    for (final SessionState value in SessionState.values) {
      if (value.wire == raw) return value;
    }
    return SessionState.unknown;
  }
}

/// How the Mac can reach a session: its own pty (console), a background job (job), or not at all (viewOnly).
enum SessionControl {
  console('console'),
  job('job'),
  viewOnly('viewOnly'),
  unknown('unknown');

  const SessionControl(this.wire);

  /// The value on the wire.
  final String wire;

  /// The case for a wire value; [unknown] for one this version does not know.
  static SessionControl parse(String raw) {
    for (final SessionControl value in SessionControl.values) {
      if (value.wire == raw) return value;
    }
    return SessionControl.unknown;
  }
}

/// Who wrote a chat message. Tool calls of one step arrive as one tools message.
enum ChatRole {
  user('user'),
  assistant('assistant'),
  tools('tools'),
  unknown('unknown');

  const ChatRole(this.wire);

  /// The value on the wire.
  final String wire;

  /// The case for a wire value; [unknown] for one this version does not know.
  static ChatRole parse(String raw) {
    for (final ChatRole value in ChatRole.values) {
      if (value.wire == raw) return value;
    }
    return ChatRole.unknown;
  }
}

/// A `!` shell command (text is the command, output what it printed) or the notice that the conversation was compacted. Absent for a normal message.
enum ChatKind {
  shell('shell'),
  compaction('compaction'),
  unknown('unknown');

  const ChatKind(this.wire);

  /// The value on the wire.
  final String wire;

  /// The case for a wire value; [unknown] for one this version does not know.
  static ChatKind parse(String raw) {
    for (final ChatKind value in ChatKind.values) {
      if (value.wire == raw) return value;
    }
    return ChatKind.unknown;
  }
}

/// A permission (allow or deny) or a question with options.
enum PromptKind {
  permission('permission'),
  question('question'),
  unknown('unknown');

  const PromptKind(this.wire);

  /// The value on the wire.
  final String wire;

  /// The case for a wire value; [unknown] for one this version does not know.
  static PromptKind parse(String raw) {
    for (final PromptKind value in PromptKind.values) {
      if (value.wire == raw) return value;
    }
    return PromptKind.unknown;
  }
}

/// Whether the subagent is still working.
enum SubagentState {
  running('running'),
  done('done'),
  unknown('unknown');

  const SubagentState(this.wire);

  /// The value on the wire.
  final String wire;

  /// The case for a wire value; [unknown] for one this version does not know.
  static SubagentState parse(String raw) {
    for (final SubagentState value in SubagentState.values) {
      if (value.wire == raw) return value;
    }
    return SubagentState.unknown;
  }
}

/// Where a chat picture came from.
enum ImageOrigin {
  user('user'),
  toolResult('toolResult'),
  file('file'),
  unknown('unknown');

  const ImageOrigin(this.wire);

  /// The value on the wire.
  final String wire;

  /// The case for a wire value; [unknown] for one this version does not know.
  static ImageOrigin parse(String raw) {
    for (final ImageOrigin value in ImageOrigin.values) {
      if (value.wire == raw) return value;
    }
    return ImageOrigin.unknown;
  }
}

/// Which tab of the viz bundle a spec fills.
enum VizKind {
  flow('flow'),
  diagram('diagram'),
  entity('entity'),
  chart('chart'),
  unknown('unknown');

  const VizKind(this.wire);

  /// The value on the wire.
  final String wire;

  /// The case for a wire value; [unknown] for one this version does not know.
  static VizKind parse(String raw) {
    for (final VizKind value in VizKind.values) {
      if (value.wire == raw) return value;
    }
    return VizKind.unknown;
  }
}

/// A subagent a session started.
class Subagent {
  const Subagent({
    required this.id,
    required this.type,
    required this.description,
    required this.model,
    required this.depth,
    this.finished = false,
    required this.state,
    required this.lastActivity,
  });

  factory Subagent.fromJson(JsonMap json) => Subagent(
    id: _need(json, 'id', _asString),
    type: _need(json, 'type', _asString),
    description: _need(json, 'description', _asString),
    model: _need(json, 'model', _asString),
    depth: _need(json, 'depth', _asInt),
    finished: _maybe(json, 'finished', _asBool) ?? false,
    state: _need(json, 'state', (Object? v) => SubagentState.parse(_asString(v))),
    lastActivity: _need(json, 'lastActivity', _asDouble),
  );

  JsonMap toJson() => <String, Object?>{
    'id': id,
    'type': type,
    'description': description,
    'model': model,
    'depth': depth,
    'finished': finished,
    'state': state.wire,
    'lastActivity': lastActivity,
  };

  final String id;

  /// The agent type, for example Explore.
  final String type;
  final String description;
  final String model;

  /// 1 for a subagent of the session, 2 for one of its subagents.
  final int depth;
  final bool finished;
  final SubagentState state;

  /// Milliseconds since 1970-01-01 UTC.
  final double lastActivity;
}

/// One question of an AskUserQuestion prompt.
class PromptQuestion {
  const PromptQuestion({
    required this.header,
    required this.question,
    required this.options,
    required this.multiSelect,
  });

  factory PromptQuestion.fromJson(JsonMap json) => PromptQuestion(
    header: _need(json, 'header', _asString),
    question: _need(json, 'question', _asString),
    options: _need(json, 'options', (Object? v) => _asList<String>(v, _asString)),
    multiSelect: _need(json, 'multiSelect', _asBool),
  );

  JsonMap toJson() => <String, Object?>{
    'header': header,
    'question': question,
    'options': options,
    'multiSelect': multiSelect,
  };

  final String header;
  final String question;
  final List<String> options;
  final bool multiSelect;
}

/// A question or permission a hook on the Mac is waiting on.
class Prompt {
  const Prompt({
    required this.requestId,
    required this.sessionId,
    required this.kind,
    required this.toolName,
    required this.summary,
    required this.questions,
    required this.created,
  });

  factory Prompt.fromJson(JsonMap json) => Prompt(
    requestId: _need(json, 'requestId', _asString),
    sessionId: _need(json, 'sessionId', _asString),
    kind: _need(json, 'kind', (Object? v) => PromptKind.parse(_asString(v))),
    toolName: _need(json, 'toolName', _asString),
    summary: _need(json, 'summary', _asString),
    questions: _need(json, 'questions', (Object? v) => _asList<PromptQuestion>(v, (Object? e) => PromptQuestion.fromJson(_asMap(e)))),
    created: _need(json, 'created', _asDouble),
  );

  JsonMap toJson() => <String, Object?>{
    'requestId': requestId,
    'sessionId': sessionId,
    'kind': kind.wire,
    'toolName': toolName,
    'summary': summary,
    'questions': <Object?>[for (final PromptQuestion e in questions) e.toJson()],
    'created': created,
  };

  final String requestId;
  final String sessionId;
  final PromptKind kind;
  final String toolName;
  final String summary;

  /// Empty for a permission.
  final List<PromptQuestion> questions;

  /// Milliseconds since 1970-01-01 UTC.
  final double created;
}

/// Whether the Mac can take a view-only session over. Only on view-only sessions.
class Takeover {
  const Takeover({
    required this.available,
    this.reason,
    this.stoppable = false,
    this.busy = false,
  });

  factory Takeover.fromJson(JsonMap json) => Takeover(
    available: _need(json, 'available', _asBool),
    reason: _maybe(json, 'reason', _asString),
    stoppable: _maybe(json, 'stoppable', _asBool) ?? false,
    busy: _maybe(json, 'busy', _asBool) ?? false,
  );

  JsonMap toJson() => <String, Object?>{
    'available': available,
    if (reason != null) 'reason': reason,
    'stoppable': stoppable,
    'busy': busy,
  };

  /// Can be resumed right now.
  final bool available;

  /// Why not, when it can't.
  final String? reason;

  /// The original is still running but the Mac can stop it (session.takeover with stopOriginal) and resume.
  final bool stoppable;

  /// The original is mid-turn: stopping it interrupts the turn.
  final bool busy;
}

/// A cheap marker of Claude's newest reply. A new id means a new reply.
class LastMessage {
  const LastMessage({
    required this.id,
    required this.text,
  });

  factory LastMessage.fromJson(JsonMap json) => LastMessage(
    id: _need(json, 'id', _asString),
    text: _need(json, 'text', _asString),
  );

  JsonMap toJson() => <String, Object?>{
    'id': id,
    'text': text,
  };

  final String id;

  /// The first lines of the reply.
  final String text;
}

/// One limit of the account a session runs on, as a percentage used.
class Limit {
  const Limit({
    required this.label,
    required this.percent,
    this.resetsAt,
  });

  factory Limit.fromJson(JsonMap json) => Limit(
    label: _need(json, 'label', _asString),
    percent: _need(json, 'percent', _asDouble),
    resetsAt: _maybe(json, 'resetsAt', _asDouble),
  );

  JsonMap toJson() => <String, Object?>{
    'label': label,
    'percent': percent,
    if (resetsAt != null) 'resetsAt': resetsAt,
  };

  /// For example 5h or Week.
  final String label;
  final double percent;

  /// Milliseconds since 1970-01-01 UTC.
  final double? resetsAt;
}

/// What the chat header shows: the context in use, the model's window and the account's limits.
class Facts {
  const Facts({
    this.contextTokens,
    this.contextWindow,
    this.model,
    required this.limits,
  });

  factory Facts.fromJson(JsonMap json) => Facts(
    contextTokens: _maybe(json, 'contextTokens', _asInt),
    contextWindow: _maybe(json, 'contextWindow', _asInt),
    model: _maybe(json, 'model', _asString),
    limits: _need(json, 'limits', (Object? v) => _asList<Limit>(v, (Object? e) => Limit.fromJson(_asMap(e)))),
  );

  JsonMap toJson() => <String, Object?>{
    if (contextTokens != null) 'contextTokens': contextTokens,
    if (contextWindow != null) 'contextWindow': contextWindow,
    if (model != null) 'model': model,
    'limits': <Object?>[for (final Limit e in limits) e.toJson()],
  };

  final int? contextTokens;

  /// Absent when the window is unknown.
  final int? contextWindow;
  final String? model;
  final List<Limit> limits;
}

/// One session the Mac knows about.
class Session {
  const Session({
    required this.id,
    required this.name,
    required this.project,
    required this.cwd,
    required this.entrypoint,
    required this.state,
    required this.control,
    required this.statusUpdatedAt,
    required this.startedAt,
    required this.lastActivity,
    this.subagents = const <Subagent>[],
    this.pending,
    this.takeover,
    this.activity,
    this.facts,
    this.lastMessage,
    this.tool = 'claude',
  });

  factory Session.fromJson(JsonMap json) => Session(
    id: _need(json, 'id', _asString),
    name: _need(json, 'name', _asString),
    project: _need(json, 'project', _asString),
    cwd: _need(json, 'cwd', _asString),
    entrypoint: _need(json, 'entrypoint', _asString),
    state: _need(json, 'state', (Object? v) => SessionState.parse(_asString(v))),
    control: _need(json, 'control', (Object? v) => SessionControl.parse(_asString(v))),
    statusUpdatedAt: _need(json, 'statusUpdatedAt', _asDouble),
    startedAt: _need(json, 'startedAt', _asDouble),
    lastActivity: _need(json, 'lastActivity', _asDouble),
    subagents: _maybe(json, 'subagents', (Object? v) => _asList<Subagent>(v, (Object? e) => Subagent.fromJson(_asMap(e)))) ?? const <Subagent>[],
    pending: _maybe(json, 'pending', (Object? v) => Prompt.fromJson(_asMap(v))),
    takeover: _maybe(json, 'takeover', (Object? v) => Takeover.fromJson(_asMap(v))),
    activity: _maybe(json, 'activity', _asString),
    facts: _maybe(json, 'facts', (Object? v) => Facts.fromJson(_asMap(v))),
    lastMessage: _maybe(json, 'lastMessage', (Object? v) => LastMessage.fromJson(_asMap(v))),
    tool: _maybe(json, 'tool', _asString) ?? 'claude',
  );

  JsonMap toJson() => <String, Object?>{
    'id': id,
    'name': name,
    'project': project,
    'cwd': cwd,
    'entrypoint': entrypoint,
    'state': state.wire,
    'control': control.wire,
    'statusUpdatedAt': statusUpdatedAt,
    'startedAt': startedAt,
    'lastActivity': lastActivity,
    'subagents': <Object?>[for (final Subagent e in subagents) e.toJson()],
    if (pending != null) 'pending': pending!.toJson(),
    if (takeover != null) 'takeover': takeover!.toJson(),
    if (activity != null) 'activity': activity,
    if (facts != null) 'facts': facts!.toJson(),
    if (lastMessage != null) 'lastMessage': lastMessage!.toJson(),
    'tool': tool,
  };

  final String id;
  final String name;

  /// Last path component of cwd.
  final String project;
  final String cwd;

  /// cli, desktop, job, sdk, or the raw entrypoint the session reported.
  final String entrypoint;
  final SessionState state;
  final SessionControl control;

  /// Milliseconds since 1970-01-01 UTC.
  final double statusUpdatedAt;

  /// Milliseconds since 1970-01-01 UTC.
  final double startedAt;

  /// Milliseconds since 1970-01-01 UTC.
  final double lastActivity;
  final List<Subagent> subagents;
  final Prompt? pending;
  final Takeover? takeover;

  /// What a working session is doing, for example Editing main.swift.
  final String? activity;
  final Facts? facts;
  final LastMessage? lastMessage;

  /// claude, codex or agy. Additive: an older Mac sends none, an older phone ignores it.
  final String tool;
}

/// A picture a chat message refers to. Only this description travels in the chat; the bytes come from chat.image.
class Image {
  const Image({
    required this.id,
    required this.mediaType,
    this.width,
    this.height,
    required this.origin,
    this.bytes = 0,
  });

  factory Image.fromJson(JsonMap json) => Image(
    id: _need(json, 'id', _asString),
    mediaType: _need(json, 'mediaType', _asString),
    width: _maybe(json, 'width', _asInt),
    height: _maybe(json, 'height', _asInt),
    origin: _need(json, 'origin', (Object? v) => ImageOrigin.parse(_asString(v))),
    bytes: _maybe(json, 'bytes', _asInt) ?? 0,
  );

  JsonMap toJson() => <String, Object?>{
    'id': id,
    'mediaType': mediaType,
    if (width != null) 'width': width,
    if (height != null) 'height': height,
    'origin': origin.wire,
    'bytes': bytes,
  };

  final String id;
  final String mediaType;
  final int? width;
  final int? height;
  final ImageOrigin origin;

  /// Size of the original.
  final int bytes;
}

/// One tool call inside a tools message.
class ToolStep {
  const ToolStep({
    required this.name,
    required this.summary,
    this.output,
    this.path,
    this.diff,
  });

  factory ToolStep.fromJson(JsonMap json) => ToolStep(
    name: _need(json, 'name', _asString),
    summary: _need(json, 'summary', _asString),
    output: _maybe(json, 'output', _asString),
    path: _maybe(json, 'path', _asString),
    diff: _maybe(json, 'diff', (Object? v) => _asList<String>(v, _asString)),
  );

  JsonMap toJson() => <String, Object?>{
    'name': name,
    'summary': summary,
    if (output != null) 'output': output,
    if (path != null) 'path': path,
    if (diff != null) 'diff': diff,
  };

  final String name;
  final String summary;
  final String? output;
  final String? path;

  /// One string per row: "+added", "-removed", " context". Absent when there is no diff.
  final List<String>? diff;
}

/// One message of a conversation.
class ChatMessage {
  const ChatMessage({
    required this.id,
    required this.role,
    required this.text,
    required this.time,
    this.toolSummary,
    this.queued = false,
    this.images = const <Image>[],
    this.kind,
    this.steps,
    this.output,
  });

  factory ChatMessage.fromJson(JsonMap json) => ChatMessage(
    id: _need(json, 'id', _asString),
    role: _need(json, 'role', (Object? v) => ChatRole.parse(_asString(v))),
    text: _need(json, 'text', _asString),
    time: _need(json, 'time', _asDouble),
    toolSummary: _maybe(json, 'toolSummary', _asString),
    queued: _maybe(json, 'queued', _asBool) ?? false,
    images: _maybe(json, 'images', (Object? v) => _asList<Image>(v, (Object? e) => Image.fromJson(_asMap(e)))) ?? const <Image>[],
    kind: _maybe(json, 'kind', (Object? v) => ChatKind.parse(_asString(v))),
    steps: _maybe(json, 'steps', (Object? v) => _asList<ToolStep>(v, (Object? e) => ToolStep.fromJson(_asMap(e)))),
    output: _maybe(json, 'output', _asString),
  );

  JsonMap toJson() => <String, Object?>{
    'id': id,
    'role': role.wire,
    'text': text,
    'time': time,
    if (toolSummary != null) 'toolSummary': toolSummary,
    'queued': queued,
    'images': <Object?>[for (final Image e in images) e.toJson()],
    if (kind != null) 'kind': kind!.wire,
    if (steps != null) 'steps': <Object?>[for (final ToolStep e in steps!) e.toJson()],
    if (output != null) 'output': output,
  };

  final String id;
  final ChatRole role;
  final String text;

  /// Milliseconds since 1970-01-01 UTC.
  final double time;

  /// On tools messages: for example Read 2 files, ran 1 shell command.
  final String? toolSummary;

  /// Sent while the agent was working and not yet taken by it.
  final bool queued;
  final List<Image> images;
  final ChatKind? kind;

  /// On tools messages: the calls with input, output and diff. Absent when there are none.
  final List<ToolStep>? steps;

  /// On shell messages: what the command printed.
  final String? output;
}

/// A slash command a session can run.
class Command {
  const Command({
    required this.name,
    required this.description,
    this.argumentHint,
  });

  factory Command.fromJson(JsonMap json) => Command(
    name: _need(json, 'name', _asString),
    description: _need(json, 'description', _asString),
    argumentHint: _maybe(json, 'argumentHint', _asString),
  );

  JsonMap toJson() => <String, Object?>{
    'name': name,
    'description': description,
    if (argumentHint != null) 'argumentHint': argumentHint,
  };

  /// Without the leading slash.
  final String name;
  final String description;
  final String? argumentHint;
}

/// An empty-state message.
class VizMessage {
  const VizMessage({
    required this.title,
    this.detail,
  });

  factory VizMessage.fromJson(JsonMap json) => VizMessage(
    title: _need(json, 'title', _asString),
    detail: _maybe(json, 'detail', _asString),
  );

  JsonMap toJson() => <String, Object?>{
    'title': title,
    if (detail != null) 'detail': detail,
  };

  final String title;
  final String? detail;
}

/// Why a spec could not be built.
class VizFailure {
  const VizFailure({
    required this.title,
    required this.message,
    this.source,
  });

  factory VizFailure.fromJson(JsonMap json) => VizFailure(
    title: _need(json, 'title', _asString),
    message: _need(json, 'message', _asString),
    source: _maybe(json, 'source', _asString),
  );

  JsonMap toJson() => <String, Object?>{
    'title': title,
    'message': message,
    if (source != null) 'source': source,
  };

  final String title;
  final String message;
  final String? source;
}

/// One tool call in a flow.
class FlowCall {
  const FlowCall({
    required this.tool,
    required this.target,
    required this.subagent,
    required this.edit,
    this.time,
  });

  factory FlowCall.fromJson(JsonMap json) => FlowCall(
    tool: _need(json, 'tool', _asString),
    target: _need(json, 'target', _asString),
    subagent: _need(json, 'subagent', _asBool),
    edit: _need(json, 'edit', _asBool),
    time: _maybe(json, 'time', _asDouble),
  );

  JsonMap toJson() => <String, Object?>{
    'tool': tool,
    'target': target,
    'subagent': subagent,
    'edit': edit,
    if (time != null) 'time': time,
  };

  final String tool;
  final String target;
  final bool subagent;
  final bool edit;

  /// Milliseconds since 1970-01-01 UTC.
  final double? time;
}

/// One user turn with its calls.
class FlowTurn {
  const FlowTurn({
    required this.index,
    required this.prompt,
    required this.calls,
  });

  factory FlowTurn.fromJson(JsonMap json) => FlowTurn(
    index: _need(json, 'index', _asInt),
    prompt: _need(json, 'prompt', _asString),
    calls: _need(json, 'calls', (Object? v) => _asList<FlowCall>(v, (Object? e) => FlowCall.fromJson(_asMap(e)))),
  );

  JsonMap toJson() => <String, Object?>{
    'index': index,
    'prompt': prompt,
    'calls': <Object?>[for (final FlowCall e in calls) e.toJson()],
  };

  final int index;
  final String prompt;
  final List<FlowCall> calls;
}

/// A file the session touched.
class FlowFile {
  const FlowFile({
    required this.path,
    required this.name,
    required this.count,
  });

  factory FlowFile.fromJson(JsonMap json) => FlowFile(
    path: _need(json, 'path', _asString),
    name: _need(json, 'name', _asString),
    count: _need(json, 'count', _asInt),
  );

  JsonMap toJson() => <String, Object?>{
    'path': path,
    'name': name,
    'count': count,
  };

  final String path;
  final String name;
  final int count;
}

/// The flow tab: what the session did.
class FlowSpec {
  const FlowSpec({
    this.branch,
    this.diffStat,
    required this.files,
    required this.turns,
    required this.totalCalls,
  });

  factory FlowSpec.fromJson(JsonMap json) => FlowSpec(
    branch: _maybe(json, 'branch', _asString),
    diffStat: _maybe(json, 'diffStat', _asString),
    files: _need(json, 'files', (Object? v) => _asList<FlowFile>(v, (Object? e) => FlowFile.fromJson(_asMap(e)))),
    turns: _need(json, 'turns', (Object? v) => _asList<FlowTurn>(v, (Object? e) => FlowTurn.fromJson(_asMap(e)))),
    totalCalls: _need(json, 'totalCalls', _asInt),
  );

  JsonMap toJson() => <String, Object?>{
    if (branch != null) 'branch': branch,
    if (diffStat != null) 'diffStat': diffStat,
    'files': <Object?>[for (final FlowFile e in files) e.toJson()],
    'turns': <Object?>[for (final FlowTurn e in turns) e.toJson()],
    'totalCalls': totalCalls,
  };

  final String? branch;
  final String? diffStat;
  final List<FlowFile> files;
  final List<FlowTurn> turns;
  final int totalCalls;
}

/// A diagram the conversation produced.
class DiagramBlock {
  const DiagramBlock({
    required this.id,
    required this.label,
    required this.source,
    required this.origin,
    this.time,
  });

  factory DiagramBlock.fromJson(JsonMap json) => DiagramBlock(
    id: _need(json, 'id', _asString),
    label: _need(json, 'label', _asString),
    source: _need(json, 'source', _asString),
    origin: _need(json, 'origin', _asString),
    time: _maybe(json, 'time', _asDouble),
  );

  JsonMap toJson() => <String, Object?>{
    'id': id,
    'label': label,
    'source': source,
    'origin': origin,
    if (time != null) 'time': time,
  };

  final String id;
  final String label;

  /// Mermaid source.
  final String source;
  final String origin;

  /// Milliseconds since 1970-01-01 UTC.
  final double? time;
}

/// The diagram tab.
class DiagramSpec {
  const DiagramSpec({
    required this.blocks,
  });

  factory DiagramSpec.fromJson(JsonMap json) => DiagramSpec(
    blocks: _need(json, 'blocks', (Object? v) => _asList<DiagramBlock>(v, (Object? e) => DiagramBlock.fromJson(_asMap(e)))),
  );

  JsonMap toJson() => <String, Object?>{
    'blocks': <Object?>[for (final DiagramBlock e in blocks) e.toJson()],
  };

  /// Newest first.
  final List<DiagramBlock> blocks;
}

/// A schema file.
class EntityFile {
  const EntityFile({
    required this.path,
    required this.tables,
  });

  factory EntityFile.fromJson(JsonMap json) => EntityFile(
    path: _need(json, 'path', _asString),
    tables: _need(json, 'tables', _asInt),
  );

  JsonMap toJson() => <String, Object?>{
    'path': path,
    'tables': tables,
  };

  final String path;
  final int tables;
}

/// A table of the entity diagram.
class EntityTable {
  const EntityTable({
    required this.id,
    required this.name,
    required this.columns,
  });

  factory EntityTable.fromJson(JsonMap json) => EntityTable(
    id: _need(json, 'id', _asString),
    name: _need(json, 'name', _asString),
    columns: _need(json, 'columns', _asInt),
  );

  JsonMap toJson() => <String, Object?>{
    'id': id,
    'name': name,
    'columns': columns,
  };

  /// Identifier as drawn in the diagram.
  final String id;
  final String name;
  final int columns;
}

/// A relation between tables, in the order of the diagram's lines.
class EntityLink {
  const EntityLink({
    required this.from,
    required this.to,
    required this.inferred,
  });

  factory EntityLink.fromJson(JsonMap json) => EntityLink(
    from: _need(json, 'from', _asString),
    to: _need(json, 'to', _asString),
    inferred: _need(json, 'inferred', _asBool),
  );

  JsonMap toJson() => <String, Object?>{
    'from': from,
    'to': to,
    'inferred': inferred,
  };

  final String from;
  final String to;
  final bool inferred;
}

/// The entity tab: a database schema as a Mermaid ER diagram.
class EntitySpec {
  const EntitySpec({
    required this.mermaid,
    required this.tables,
    required this.links,
    required this.files,
    required this.tableCount,
    required this.relationCount,
    required this.inferredCount,
    required this.warnings,
  });

  factory EntitySpec.fromJson(JsonMap json) => EntitySpec(
    mermaid: _need(json, 'mermaid', _asString),
    tables: _need(json, 'tables', (Object? v) => _asList<EntityTable>(v, (Object? e) => EntityTable.fromJson(_asMap(e)))),
    links: _need(json, 'links', (Object? v) => _asList<EntityLink>(v, (Object? e) => EntityLink.fromJson(_asMap(e)))),
    files: _need(json, 'files', (Object? v) => _asList<EntityFile>(v, (Object? e) => EntityFile.fromJson(_asMap(e)))),
    tableCount: _need(json, 'tableCount', _asInt),
    relationCount: _need(json, 'relationCount', _asInt),
    inferredCount: _need(json, 'inferredCount', _asInt),
    warnings: _need(json, 'warnings', (Object? v) => _asList<String>(v, _asString)),
  );

  JsonMap toJson() => <String, Object?>{
    'mermaid': mermaid,
    'tables': <Object?>[for (final EntityTable e in tables) e.toJson()],
    'links': <Object?>[for (final EntityLink e in links) e.toJson()],
    'files': <Object?>[for (final EntityFile e in files) e.toJson()],
    'tableCount': tableCount,
    'relationCount': relationCount,
    'inferredCount': inferredCount,
    'warnings': warnings,
  };

  final String mermaid;
  final List<EntityTable> tables;
  final List<EntityLink> links;
  final List<EntityFile> files;
  final int tableCount;
  final int relationCount;
  final int inferredCount;
  final List<String> warnings;
}

/// Token use at one moment.
class UsagePoint {
  const UsagePoint({
    required this.time,
    required this.input,
    required this.output,
    required this.cacheCreation,
    required this.cacheRead,
  });

  factory UsagePoint.fromJson(JsonMap json) => UsagePoint(
    time: _need(json, 'time', _asDouble),
    input: _need(json, 'input', _asInt),
    output: _need(json, 'output', _asInt),
    cacheCreation: _need(json, 'cacheCreation', _asInt),
    cacheRead: _need(json, 'cacheRead', _asInt),
  );

  JsonMap toJson() => <String, Object?>{
    'time': time,
    'input': input,
    'output': output,
    'cacheCreation': cacheCreation,
    'cacheRead': cacheRead,
  };

  /// Milliseconds since 1970-01-01 UTC.
  final double time;
  final int input;
  final int output;
  final int cacheCreation;
  final int cacheRead;
}

/// Token use over the session.
class UsageTotals {
  const UsageTotals({
    required this.input,
    required this.output,
    required this.cacheCreation,
    required this.cacheRead,
  });

  factory UsageTotals.fromJson(JsonMap json) => UsageTotals(
    input: _need(json, 'input', _asInt),
    output: _need(json, 'output', _asInt),
    cacheCreation: _need(json, 'cacheCreation', _asInt),
    cacheRead: _need(json, 'cacheRead', _asInt),
  );

  JsonMap toJson() => <String, Object?>{
    'input': input,
    'output': output,
    'cacheCreation': cacheCreation,
    'cacheRead': cacheRead,
  };

  final int input;
  final int output;
  final int cacheCreation;
  final int cacheRead;
}

/// How often a tool ran.
class ChartCount {
  const ChartCount({
    required this.name,
    required this.count,
  });

  factory ChartCount.fromJson(JsonMap json) => ChartCount(
    name: _need(json, 'name', _asString),
    count: _need(json, 'count', _asInt),
  );

  JsonMap toJson() => <String, Object?>{
    'name': name,
    'count': count,
  };

  final String name;
  final int count;
}

/// How many requests a model served.
class ModelCount {
  const ModelCount({
    required this.model,
    required this.count,
  });

  factory ModelCount.fromJson(JsonMap json) => ModelCount(
    model: _need(json, 'model', _asString),
    count: _need(json, 'count', _asInt),
  );

  JsonMap toJson() => <String, Object?>{
    'model': model,
    'count': count,
  };

  final String model;
  final int count;
}

/// The chart tab.
class ChartSpec {
  const ChartSpec({
    required this.tokens,
    required this.totals,
    required this.tools,
    required this.models,
  });

  factory ChartSpec.fromJson(JsonMap json) => ChartSpec(
    tokens: _need(json, 'tokens', (Object? v) => _asList<UsagePoint>(v, (Object? e) => UsagePoint.fromJson(_asMap(e)))),
    totals: _need(json, 'totals', (Object? v) => UsageTotals.fromJson(_asMap(v))),
    tools: _need(json, 'tools', (Object? v) => _asList<ChartCount>(v, (Object? e) => ChartCount.fromJson(_asMap(e)))),
    models: _need(json, 'models', (Object? v) => _asList<ModelCount>(v, (Object? e) => ModelCount.fromJson(_asMap(e)))),
  );

  JsonMap toJson() => <String, Object?>{
    'tokens': <Object?>[for (final UsagePoint e in tokens) e.toJson()],
    'totals': totals.toJson(),
    'tools': <Object?>[for (final ChartCount e in tools) e.toJson()],
    'models': <Object?>[for (final ModelCount e in models) e.toJson()],
  };

  final List<UsagePoint> tokens;
  final UsageTotals totals;
  final List<ChartCount> tools;
  final List<ModelCount> models;
}

/// What the shared viz bundle renders for one tab. Only the field named by kind is filled, unless empty or error says why not.
class VizSpec {
  const VizSpec({
    required this.kind,
    this.empty,
    this.error,
    this.flow,
    this.diagram,
    this.entity,
    this.chart,
  });

  factory VizSpec.fromJson(JsonMap json) => VizSpec(
    kind: _need(json, 'kind', (Object? v) => VizKind.parse(_asString(v))),
    empty: _maybe(json, 'empty', (Object? v) => VizMessage.fromJson(_asMap(v))),
    error: _maybe(json, 'error', (Object? v) => VizFailure.fromJson(_asMap(v))),
    flow: _maybe(json, 'flow', (Object? v) => FlowSpec.fromJson(_asMap(v))),
    diagram: _maybe(json, 'diagram', (Object? v) => DiagramSpec.fromJson(_asMap(v))),
    entity: _maybe(json, 'entity', (Object? v) => EntitySpec.fromJson(_asMap(v))),
    chart: _maybe(json, 'chart', (Object? v) => ChartSpec.fromJson(_asMap(v))),
  );

  JsonMap toJson() => <String, Object?>{
    'kind': kind.wire,
    if (empty != null) 'empty': empty!.toJson(),
    if (error != null) 'error': error!.toJson(),
    if (flow != null) 'flow': flow!.toJson(),
    if (diagram != null) 'diagram': diagram!.toJson(),
    if (entity != null) 'entity': entity!.toJson(),
    if (chart != null) 'chart': chart!.toJson(),
  };

  final VizKind kind;
  final VizMessage? empty;
  final VizFailure? error;
  final FlowSpec? flow;
  final DiagramSpec? diagram;
  final EntitySpec? entity;
  final ChartSpec? chart;
}

/// One button an inbox item offers.
class InboxAction {
  const InboxAction({
    required this.id,
    required this.label,
    required this.primary,
  });

  factory InboxAction.fromJson(JsonMap json) => InboxAction(
    id: _need(json, 'id', _asString),
    label: _need(json, 'label', _asString),
    primary: _need(json, 'primary', _asBool),
  );

  JsonMap toJson() => <String, Object?>{
    'id': id,
    'label': label,
    'primary': primary,
  };

  /// One of allow, allow_once, allow_always, deny, continue, cancel, done, open. open is for the Mac only: the phone shows it disabled and the Mac refuses it.
  final String id;
  final String label;

  /// The main button of the item.
  final bool primary;
}

/// One notification the Mac's inbox holds.
class InboxItem {
  const InboxItem({
    required this.id,
    required this.kind,
    required this.title,
    required this.detail,
    this.source,
    required this.createdMs,
    this.expiresMs,
    this.closedMs,
    required this.actions,
    required this.state,
    this.answer,
    this.answeredBy,
    required this.seen,
  });

  factory InboxItem.fromJson(JsonMap json) => InboxItem(
    id: _need(json, 'id', _asString),
    kind: _need(json, 'kind', _asString),
    title: _need(json, 'title', _asString),
    detail: _need(json, 'detail', _asString),
    source: _maybe(json, 'source', (Object? v) => InboxItemSource.fromJson(_asMap(v))),
    createdMs: _need(json, 'createdMs', _asInt),
    expiresMs: _maybe(json, 'expiresMs', _asInt),
    closedMs: _maybe(json, 'closedMs', _asInt),
    actions: _need(json, 'actions', (Object? v) => _asList<InboxAction>(v, (Object? e) => InboxAction.fromJson(_asMap(e)))),
    state: _need(json, 'state', _asString),
    answer: _maybe(json, 'answer', _asString),
    answeredBy: _maybe(json, 'answeredBy', _asString),
    seen: _need(json, 'seen', _asBool),
  );

  JsonMap toJson() => <String, Object?>{
    'id': id,
    'kind': kind,
    'title': title,
    'detail': detail,
    if (source != null) 'source': source!.toJson(),
    'createdMs': createdMs,
    if (expiresMs != null) 'expiresMs': expiresMs,
    if (closedMs != null) 'closedMs': closedMs,
    'actions': <Object?>[for (final InboxAction e in actions) e.toJson()],
    'state': state,
    if (answer != null) 'answer': answer,
    if (answeredBy != null) 'answeredBy': answeredBy,
    'seen': seen,
  };

  final String id;

  /// secret_approval, secret_request, limit_notice, action_failed or notice; a phone treats a kind it doesn't know as notice.
  final String kind;
  final String title;
  final String detail;

  /// Where the item came from.
  final InboxItemSource? source;
  final int createdMs;

  /// When an open item stops waiting.
  final int? expiresMs;

  /// When the item was answered, expired or withdrawn.
  final int? closedMs;
  final List<InboxAction> actions;

  /// open, answered, expired or withdrawn.
  final String state;

  /// The action id that answered it.
  final String? answer;

  /// mac or phone.
  final String? answeredBy;
  final bool seen;
}

/// Where the item came from.
class InboxItemSource {
  const InboxItemSource({
    this.node,
    this.session,
    this.job,
  });

  factory InboxItemSource.fromJson(JsonMap json) => InboxItemSource(
    node: _maybe(json, 'node', _asString),
    session: _maybe(json, 'session', _asString),
    job: _maybe(json, 'job', _asString),
  );

  JsonMap toJson() => <String, Object?>{
    if (node != null) 'node': node,
    if (session != null) 'session': session,
    if (job != null) 'job': job,
  };

  final String? node;
  final String? session;
  final String? job;
}

/// First message on a connection from a paired phone.
class HelloRequest {
  const HelloRequest({
    this.id,
    this.op = 'hello',
    required this.token,
  });

  factory HelloRequest.fromJson(JsonMap json) => HelloRequest(
    id: json['id'],
    op: _need(json, 'op', _asString),
    token: _need(json, 'token', _asString),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'op': op,
    'token': token,
  };

  final Object? id;
  final String op;

  /// The device token pair returned.
  final String token;
}

/// First message from a phone that is pairing with the one-time code; the Mac shows its approval sheet.
class PairRequest {
  const PairRequest({
    this.id,
    this.op = 'pair',
    required this.code,
    required this.device,
  });

  factory PairRequest.fromJson(JsonMap json) => PairRequest(
    id: json['id'],
    op: _need(json, 'op', _asString),
    code: _need(json, 'code', _asString),
    device: _need(json, 'device', (Object? v) => PairRequestDevice.fromJson(_asMap(v))),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'op': op,
    'code': code,
    'device': device.toJson(),
  };

  final Object? id;
  final String op;

  /// The 8-character one-time code.
  final String code;

  /// The phone.
  final PairRequestDevice device;
}

/// The phone.
class PairRequestDevice {
  const PairRequestDevice({
    required this.name,
    this.platform,
  });

  factory PairRequestDevice.fromJson(JsonMap json) => PairRequestDevice(
    name: _need(json, 'name', _asString),
    platform: _maybe(json, 'platform', _asString),
  );

  JsonMap toJson() => <String, Object?>{
    'name': name,
    if (platform != null) 'platform': platform,
  };

  final String name;

  /// ios, android; defaults to unknown.
  final String? platform;
}

/// Keep-alive; answered with the Mac's clock.
class PingRequest {
  const PingRequest({
    this.id,
    this.op = 'ping',
  });

  factory PingRequest.fromJson(JsonMap json) => PingRequest(
    id: json['id'],
    op: _need(json, 'op', _asString),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'op': op,
  };

  final Object? id;
  final String op;
}

/// All sessions.
class SessionsListRequest {
  const SessionsListRequest({
    this.id,
    this.op = 'sessions.list',
  });

  factory SessionsListRequest.fromJson(JsonMap json) => SessionsListRequest(
    id: json['id'],
    op: _need(json, 'op', _asString),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'op': op,
  };

  final Object? id;
  final String op;
}

/// One page of a conversation: the newest limit messages, or the ones before or after a message.
class ChatGetRequest {
  const ChatGetRequest({
    this.id,
    this.op = 'chat.get',
    required this.sessionId,
    this.subagentId,
    this.limit,
    this.before,
    this.after,
  });

  factory ChatGetRequest.fromJson(JsonMap json) => ChatGetRequest(
    id: json['id'],
    op: _need(json, 'op', _asString),
    sessionId: _need(json, 'sessionId', _asString),
    subagentId: _maybe(json, 'subagentId', _asString),
    limit: _maybe(json, 'limit', _asInt),
    before: _maybe(json, 'before', _asString),
    after: _maybe(json, 'after', _asString),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'op': op,
    'sessionId': sessionId,
    if (subagentId != null) 'subagentId': subagentId,
    if (limit != null) 'limit': limit,
    if (before != null) 'before': before,
    if (after != null) 'after': after,
  };

  final Object? id;
  final String op;

  /// The session's id.
  final String sessionId;

  /// Read a subagent's conversation instead.
  final String? subagentId;

  /// 1 to 200, default 50.
  final int? limit;

  /// Id of the oldest message the phone has.
  final String? before;

  /// Id of the newest message the phone has.
  final String? after;
}

/// The viz specs of a session.
class VizGetRequest {
  const VizGetRequest({
    this.id,
    this.op = 'viz.get',
    required this.sessionId,
    this.entity,
  });

  factory VizGetRequest.fromJson(JsonMap json) => VizGetRequest(
    id: json['id'],
    op: _need(json, 'op', _asString),
    sessionId: _need(json, 'sessionId', _asString),
    entity: _maybe(json, 'entity', _asBool),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'op': op,
    'sessionId': sessionId,
    if (entity != null) 'entity': entity,
  };

  final Object? id;
  final String op;

  /// The session's id.
  final String sessionId;

  /// Also build the entity spec.
  final bool? entity;
}

/// A JPEG of a localhost page. The Mac answers with JSON, then a binary WEB1 frame.
class WebSnapshotRequest {
  const WebSnapshotRequest({
    this.id,
    this.op = 'web.snapshot',
    required this.sessionId,
    this.width,
    this.url,
  });

  factory WebSnapshotRequest.fromJson(JsonMap json) => WebSnapshotRequest(
    id: json['id'],
    op: _need(json, 'op', _asString),
    sessionId: _need(json, 'sessionId', _asString),
    width: _maybe(json, 'width', _asInt),
    url: _maybe(json, 'url', _asString),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'op': op,
    'sessionId': sessionId,
    if (width != null) 'width': width,
    if (url != null) 'url': url,
  };

  final Object? id;
  final String op;

  /// The session's id.
  final String sessionId;

  /// 100 to 1200, default 400.
  final int? width;

  /// An http(s) URL on localhost or 127.0.0.1; absent: the session's own web URL.
  final String? url;
}

/// Types a message into a session.
class SessionMessageRequest {
  const SessionMessageRequest({
    this.id,
    this.op = 'session.message',
    required this.sessionId,
    required this.text,
  });

  factory SessionMessageRequest.fromJson(JsonMap json) => SessionMessageRequest(
    id: json['id'],
    op: _need(json, 'op', _asString),
    sessionId: _need(json, 'sessionId', _asString),
    text: _need(json, 'text', _asString),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'op': op,
    'sessionId': sessionId,
    'text': text,
  };

  final Object? id;
  final String op;

  /// The session's id.
  final String sessionId;

  /// 1 to 20000 characters.
  final String text;
}

/// Interrupts the running turn so the queued message runs next.
class SessionSendNowRequest {
  const SessionSendNowRequest({
    this.id,
    this.op = 'session.sendNow',
    required this.sessionId,
  });

  factory SessionSendNowRequest.fromJson(JsonMap json) => SessionSendNowRequest(
    id: json['id'],
    op: _need(json, 'op', _asString),
    sessionId: _need(json, 'sessionId', _asString),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'op': op,
    'sessionId': sessionId,
  };

  final Object? id;
  final String op;

  /// The session's id.
  final String sessionId;
}

/// Starts a photo upload.
class SessionImageBeginRequest {
  const SessionImageBeginRequest({
    this.id,
    this.op = 'session.image.begin',
    required this.sessionId,
    required this.bytes,
    this.caption,
  });

  factory SessionImageBeginRequest.fromJson(JsonMap json) => SessionImageBeginRequest(
    id: json['id'],
    op: _need(json, 'op', _asString),
    sessionId: _need(json, 'sessionId', _asString),
    bytes: _need(json, 'bytes', _asInt),
    caption: _maybe(json, 'caption', _asString),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'op': op,
    'sessionId': sessionId,
    'bytes': bytes,
    if (caption != null) 'caption': caption,
  };

  final Object? id;
  final String op;

  /// The session's id.
  final String sessionId;

  /// Size of the photo, at most 6 MiB.
  final int bytes;
  final String? caption;
}

/// One piece of a photo, in order.
class SessionImageChunkRequest {
  const SessionImageChunkRequest({
    this.id,
    this.op = 'session.image.chunk',
    required this.uploadId,
    required this.seq,
    required this.data,
  });

  factory SessionImageChunkRequest.fromJson(JsonMap json) => SessionImageChunkRequest(
    id: json['id'],
    op: _need(json, 'op', _asString),
    uploadId: _need(json, 'uploadId', _asString),
    seq: _need(json, 'seq', _asInt),
    data: _need(json, 'data', _asString),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'op': op,
    'uploadId': uploadId,
    'seq': seq,
    'data': data,
  };

  final Object? id;
  final String op;
  final String uploadId;

  /// 0, 1, 2, ...
  final int seq;

  /// Base64 of at most 96 KiB.
  final String data;
}

/// Finishes a photo upload; its path goes to the session with the caption.
class SessionImageEndRequest {
  const SessionImageEndRequest({
    this.id,
    this.op = 'session.image.end',
    required this.uploadId,
  });

  factory SessionImageEndRequest.fromJson(JsonMap json) => SessionImageEndRequest(
    id: json['id'],
    op: _need(json, 'op', _asString),
    uploadId: _need(json, 'uploadId', _asString),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'op': op,
    'uploadId': uploadId,
  };

  final Object? id;
  final String op;
  final String uploadId;
}

/// The slash commands of a session.
class CommandsListRequest {
  const CommandsListRequest({
    this.id,
    this.op = 'commands.list',
    required this.sessionId,
  });

  factory CommandsListRequest.fromJson(JsonMap json) => CommandsListRequest(
    id: json['id'],
    op: _need(json, 'op', _asString),
    sessionId: _need(json, 'sessionId', _asString),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'op': op,
    'sessionId': sessionId,
  };

  final Object? id;
  final String op;

  /// The session's id.
  final String sessionId;
}

/// Answers a permission (allow) or a question (choice or text).
class PromptAnswerRequest {
  const PromptAnswerRequest({
    this.id,
    this.op = 'prompt.answer',
    required this.requestId,
    this.allow,
    this.choice,
    this.text,
  });

  factory PromptAnswerRequest.fromJson(JsonMap json) => PromptAnswerRequest(
    id: json['id'],
    op: _need(json, 'op', _asString),
    requestId: _need(json, 'requestId', _asString),
    allow: _maybe(json, 'allow', _asBool),
    choice: json['choice'],
    text: _maybe(json, 'text', _asString),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'op': op,
    'requestId': requestId,
    if (allow != null) 'allow': allow,
    if (choice != null) 'choice': choice,
    if (text != null) 'text': text,
  };

  final Object? id;
  final String op;
  final String requestId;

  /// For a permission.
  final bool? allow;

  /// For a question: a 1-based option number or an option label; an array with one entry per question.
  final Object? choice;

  /// A free-text answer to a single question, at most 500 characters.
  final String? text;
}

/// Starts a Claude Code session in a folder inside the Mac's home.
class SessionNewRequest {
  const SessionNewRequest({
    this.id,
    this.op = 'session.new',
    required this.cwd,
    this.model,
    this.modelId,
  });

  factory SessionNewRequest.fromJson(JsonMap json) => SessionNewRequest(
    id: json['id'],
    op: _need(json, 'op', _asString),
    cwd: _need(json, 'cwd', _asString),
    model: _maybe(json, 'model', _asString),
    modelId: _maybe(json, 'modelId', _asString),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'op': op,
    'cwd': cwd,
    if (model != null) 'model': model,
    if (modelId != null) 'modelId': modelId,
  };

  final Object? id;
  final String op;

  /// An absolute path.
  final String cwd;

  /// default, opus, sonnet or haiku.
  final String? model;

  /// A model id from the `models` of sessions.list, such as claude-opus-5-5 (a [1m] or date suffix is accepted). Wins over `model` when both are sent; the Mac answers invalid for an id it doesn't list. An older Mac ignores it.
  final String? modelId;
}

/// A JPEG of a picture in the chat. The Mac answers with JSON, then a binary IMG1 frame.
class ChatImageRequest {
  const ChatImageRequest({
    this.id,
    this.op = 'chat.image',
    required this.sessionId,
    required this.imageId,
    this.maxWidth,
  });

  factory ChatImageRequest.fromJson(JsonMap json) => ChatImageRequest(
    id: json['id'],
    op: _need(json, 'op', _asString),
    sessionId: _need(json, 'sessionId', _asString),
    imageId: _need(json, 'imageId', _asString),
    maxWidth: _maybe(json, 'maxWidth', _asInt),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'op': op,
    'sessionId': sessionId,
    'imageId': imageId,
    if (maxWidth != null) 'maxWidth': maxWidth,
  };

  final Object? id;
  final String op;

  /// The session's id.
  final String sessionId;
  final String imageId;

  /// 64 to 1600, default 800.
  final int? maxWidth;
}

/// Resumes a view-only session in a terminal the Mac owns.
class SessionTakeoverRequest {
  const SessionTakeoverRequest({
    this.id,
    this.op = 'session.takeover',
    required this.sessionId,
    this.stopOriginal,
  });

  factory SessionTakeoverRequest.fromJson(JsonMap json) => SessionTakeoverRequest(
    id: json['id'],
    op: _need(json, 'op', _asString),
    sessionId: _need(json, 'sessionId', _asString),
    stopOriginal: _maybe(json, 'stopOriginal', _asBool),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'op': op,
    'sessionId': sessionId,
    if (stopOriginal != null) 'stopOriginal': stopOriginal,
  };

  final Object? id;
  final String op;

  /// The session's id.
  final String sessionId;

  /// Stop the original claude process first.
  final bool? stopOriginal;
}

/// Folders sessions ran in recently.
class ProjectsRecentRequest {
  const ProjectsRecentRequest({
    this.id,
    this.op = 'projects.recent',
  });

  factory ProjectsRecentRequest.fromJson(JsonMap json) => ProjectsRecentRequest(
    id: json['id'],
    op: _need(json, 'op', _asString),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'op': op,
  };

  final Object? id;
  final String op;
}

/// Replaces what the Mac pushes to this connection.
class SubscribeRequest {
  const SubscribeRequest({
    this.id,
    this.op = 'subscribe',
    this.sessions,
    this.sessionId,
    this.subagentId,
    this.lastMessageId,
    this.phone,
    this.inbox,
  });

  factory SubscribeRequest.fromJson(JsonMap json) => SubscribeRequest(
    id: json['id'],
    op: _need(json, 'op', _asString),
    sessions: _maybe(json, 'sessions', _asBool),
    sessionId: _maybe(json, 'sessionId', _asString),
    subagentId: _maybe(json, 'subagentId', _asString),
    lastMessageId: _maybe(json, 'lastMessageId', _asString),
    phone: _maybe(json, 'phone', (Object? v) => SubscribeRequestPhone.fromJson(_asMap(v))),
    inbox: _maybe(json, 'inbox', _asBool),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'op': op,
    if (sessions != null) 'sessions': sessions,
    if (sessionId != null) 'sessionId': sessionId,
    if (subagentId != null) 'subagentId': subagentId,
    if (lastMessageId != null) 'lastMessageId': lastMessageId,
    if (phone != null) 'phone': phone!.toJson(),
    if (inbox != null) 'inbox': inbox,
  };

  final Object? id;
  final String op;

  /// Push the session list and prompts.
  final bool? sessions;

  /// Push new chat messages of this session.
  final String? sessionId;
  final String? subagentId;

  /// The newest message the phone already has, so the ones after it are pushed.
  final String? lastMessageId;

  /// Stream the Mac's device window as PHN1 frames.
  final SubscribeRequestPhone? phone;

  /// Push the inbox's items, and each one raised, changed or removed after. Absent means no inbox pushes.
  final bool? inbox;
}

/// Stream the Mac's device window as PHN1 frames.
class SubscribeRequestPhone {
  const SubscribeRequestPhone({
    this.deviceWindowId,
    this.fps,
    this.width,
  });

  factory SubscribeRequestPhone.fromJson(JsonMap json) => SubscribeRequestPhone(
    deviceWindowId: _maybe(json, 'deviceWindowId', _asInt),
    fps: _maybe(json, 'fps', _asInt),
    width: _maybe(json, 'width', _asInt),
  );

  JsonMap toJson() => <String, Object?>{
    if (deviceWindowId != null) 'deviceWindowId': deviceWindowId,
    if (fps != null) 'fps': fps,
    if (width != null) 'width': width,
  };

  final int? deviceWindowId;

  /// 1 to 15, default 10.
  final int? fps;

  /// 100 to 600, default 400.
  final int? width;
}

/// Stops every push.
class UnsubscribeRequest {
  const UnsubscribeRequest({
    this.id,
    this.op = 'unsubscribe',
  });

  factory UnsubscribeRequest.fromJson(JsonMap json) => UnsubscribeRequest(
    id: json['id'],
    op: _need(json, 'op', _asString),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'op': op,
  };

  final Object? id;
  final String op;
}

/// The Mac's notification inbox.
class InboxListRequest {
  const InboxListRequest({
    this.id,
    this.op = 'inbox.list',
  });

  factory InboxListRequest.fromJson(JsonMap json) => InboxListRequest(
    id: json['id'],
    op: _need(json, 'op', _asString),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'op': op,
  };

  final Object? id;
  final String op;
}

/// Presses one action of an open inbox item. The first answer wins, from the Mac or a phone.
class InboxAnswerRequest {
  const InboxAnswerRequest({
    this.id,
    this.op = 'inbox.answer',
    required this.itemId,
    required this.action,
  });

  factory InboxAnswerRequest.fromJson(JsonMap json) => InboxAnswerRequest(
    id: json['id'],
    op: _need(json, 'op', _asString),
    itemId: _need(json, 'itemId', _asString),
    action: _need(json, 'action', _asString),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'op': op,
    'itemId': itemId,
    'action': action,
  };

  final Object? id;
  final String op;

  /// The inbox item's id.
  final String itemId;

  /// An action id the item offers; open is refused.
  final String action;
}

/// The connection is authenticated.
class HelloResponse {
  const HelloResponse({
    this.id,
    this.ok = true,
    required this.data,
  });

  factory HelloResponse.fromJson(JsonMap json) => HelloResponse(
    id: json['id'],
    ok: _need(json, 'ok', _asBool),
    data: _need(json, 'data', (Object? v) => HelloResponseData.fromJson(_asMap(v))),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'ok': ok,
    'data': data.toJson(),
  };

  final Object? id;
  final bool ok;

  /// The answer.
  final HelloResponseData data;
}

/// The answer.
class HelloResponseData {
  const HelloResponseData({
    required this.device,
    required this.version,
  });

  factory HelloResponseData.fromJson(JsonMap json) => HelloResponseData(
    device: _need(json, 'device', (Object? v) => HelloResponseDataDevice.fromJson(_asMap(v))),
    version: _need(json, 'version', _asInt),
  );

  JsonMap toJson() => <String, Object?>{
    'device': device.toJson(),
    'version': version,
  };

  /// The paired phone.
  final HelloResponseDataDevice device;

  /// The bridge wire version, 1.
  final int version;
}

/// The paired phone.
class HelloResponseDataDevice {
  const HelloResponseDataDevice({
    required this.id,
    required this.name,
  });

  factory HelloResponseDataDevice.fromJson(JsonMap json) => HelloResponseDataDevice(
    id: _need(json, 'id', _asString),
    name: _need(json, 'name', _asString),
  );

  JsonMap toJson() => <String, Object?>{
    'id': id,
    'name': name,
  };

  final String id;
  final String name;
}

/// Paired: the token is shown once.
class PairResponse {
  const PairResponse({
    this.id,
    this.ok = true,
    required this.data,
  });

  factory PairResponse.fromJson(JsonMap json) => PairResponse(
    id: json['id'],
    ok: _need(json, 'ok', _asBool),
    data: _need(json, 'data', (Object? v) => PairResponseData.fromJson(_asMap(v))),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'ok': ok,
    'data': data.toJson(),
  };

  final Object? id;
  final bool ok;

  /// The answer.
  final PairResponseData data;
}

/// The answer.
class PairResponseData {
  const PairResponseData({
    required this.token,
    required this.deviceId,
    required this.version,
  });

  factory PairResponseData.fromJson(JsonMap json) => PairResponseData(
    token: _need(json, 'token', _asString),
    deviceId: _need(json, 'deviceId', _asString),
    version: _need(json, 'version', _asInt),
  );

  JsonMap toJson() => <String, Object?>{
    'token': token,
    'deviceId': deviceId,
    'version': version,
  };

  final String token;
  final String deviceId;
  final int version;
}

/// The Mac's clock.
class PingResponse {
  const PingResponse({
    this.id,
    this.ok = true,
    required this.data,
  });

  factory PingResponse.fromJson(JsonMap json) => PingResponse(
    id: json['id'],
    ok: _need(json, 'ok', _asBool),
    data: _need(json, 'data', (Object? v) => PingResponseData.fromJson(_asMap(v))),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'ok': ok,
    'data': data.toJson(),
  };

  final Object? id;
  final bool ok;

  /// The answer.
  final PingResponseData data;
}

/// The answer.
class PingResponseData {
  const PingResponseData({
    required this.time,
  });

  factory PingResponseData.fromJson(JsonMap json) => PingResponseData(
    time: _need(json, 'time', _asInt),
  );

  JsonMap toJson() => <String, Object?>{
    'time': time,
  };

  /// Milliseconds since 1970.
  final int time;
}

/// All sessions.
class SessionsListResponse {
  const SessionsListResponse({
    this.id,
    this.ok = true,
    required this.data,
  });

  factory SessionsListResponse.fromJson(JsonMap json) => SessionsListResponse(
    id: json['id'],
    ok: _need(json, 'ok', _asBool),
    data: _need(json, 'data', (Object? v) => SessionsListResponseData.fromJson(_asMap(v))),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'ok': ok,
    'data': data.toJson(),
  };

  final Object? id;
  final bool ok;

  /// The answer.
  final SessionsListResponseData data;
}

/// The answer.
class SessionsListResponseData {
  const SessionsListResponseData({
    required this.sessions,
    this.models,
  });

  factory SessionsListResponseData.fromJson(JsonMap json) => SessionsListResponseData(
    sessions: _need(json, 'sessions', (Object? v) => _asList<Session>(v, (Object? e) => Session.fromJson(_asMap(e)))),
    models: _maybe(json, 'models', (Object? v) => _asList<SessionsListResponseDataModelsItem>(v, (Object? e) => SessionsListResponseDataModelsItem.fromJson(_asMap(e)))),
  );

  JsonMap toJson() => <String, Object?>{
    'sessions': <Object?>[for (final Session e in sessions) e.toJson()],
    if (models != null) 'models': <Object?>[for (final SessionsListResponseDataModelsItem e in models!) e.toJson()],
  };

  final List<Session> sessions;

  /// The Claude models a new session may start on now, current ones only, as the Mac's catalog lists them. Absent from an older Mac; the phone then offers its short names.
  final List<SessionsListResponseDataModelsItem>? models;
}

class SessionsListResponseDataModelsItem {
  const SessionsListResponseDataModelsItem({
    required this.id,
    required this.name,
  });

  factory SessionsListResponseDataModelsItem.fromJson(JsonMap json) => SessionsListResponseDataModelsItem(
    id: _need(json, 'id', _asString),
    name: _need(json, 'name', _asString),
  );

  JsonMap toJson() => <String, Object?>{
    'id': id,
    'name': name,
  };

  /// What `modelId` of session.new takes.
  final String id;

  /// The name to show.
  final String name;
}

/// A page of messages, oldest first.
class ChatGetResponse {
  const ChatGetResponse({
    this.id,
    this.ok = true,
    required this.data,
  });

  factory ChatGetResponse.fromJson(JsonMap json) => ChatGetResponse(
    id: json['id'],
    ok: _need(json, 'ok', _asBool),
    data: _need(json, 'data', (Object? v) => ChatGetResponseData.fromJson(_asMap(v))),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'ok': ok,
    'data': data.toJson(),
  };

  final Object? id;
  final bool ok;

  /// The answer.
  final ChatGetResponseData data;
}

/// The answer.
class ChatGetResponseData {
  const ChatGetResponseData({
    required this.messages,
    required this.hasMore,
    this.reset,
  });

  factory ChatGetResponseData.fromJson(JsonMap json) => ChatGetResponseData(
    messages: _need(json, 'messages', (Object? v) => _asList<ChatMessage>(v, (Object? e) => ChatMessage.fromJson(_asMap(e)))),
    hasMore: _need(json, 'hasMore', _asBool),
    reset: _maybe(json, 'reset', _asBool),
  );

  JsonMap toJson() => <String, Object?>{
    'messages': <Object?>[for (final ChatMessage e in messages) e.toJson()],
    'hasMore': hasMore,
    if (reset != null) 'reset': reset,
  };

  final List<ChatMessage> messages;

  /// There are older messages (before) or newer ones (after).
  final bool hasMore;

  /// The after message is gone; replace what the phone has with this page.
  final bool? reset;
}

/// The viz specs.
class VizGetResponse {
  const VizGetResponse({
    this.id,
    this.ok = true,
    required this.data,
  });

  factory VizGetResponse.fromJson(JsonMap json) => VizGetResponse(
    id: json['id'],
    ok: _need(json, 'ok', _asBool),
    data: _need(json, 'data', (Object? v) => VizGetResponseData.fromJson(_asMap(v))),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'ok': ok,
    'data': data.toJson(),
  };

  final Object? id;
  final bool ok;

  /// The answer.
  final VizGetResponseData data;
}

/// The answer.
class VizGetResponseData {
  const VizGetResponseData({
    required this.suggested,
    this.webURL,
    this.webCandidates,
    this.flow,
    this.diagram,
    this.chart,
    this.entity,
  });

  factory VizGetResponseData.fromJson(JsonMap json) => VizGetResponseData(
    suggested: _need(json, 'suggested', _asString),
    webURL: _maybe(json, 'webURL', _asString),
    webCandidates: _maybe(json, 'webCandidates', (Object? v) => _asList<String>(v, _asString)),
    flow: _maybe(json, 'flow', (Object? v) => VizSpec.fromJson(_asMap(v))),
    diagram: _maybe(json, 'diagram', (Object? v) => VizSpec.fromJson(_asMap(v))),
    chart: _maybe(json, 'chart', (Object? v) => VizSpec.fromJson(_asMap(v))),
    entity: _maybe(json, 'entity', (Object? v) => VizSpec.fromJson(_asMap(v))),
  );

  JsonMap toJson() => <String, Object?>{
    'suggested': suggested,
    if (webURL != null) 'webURL': webURL,
    if (webCandidates != null) 'webCandidates': webCandidates,
    if (flow != null) 'flow': flow!.toJson(),
    if (diagram != null) 'diagram': diagram!.toJson(),
    if (chart != null) 'chart': chart!.toJson(),
    if (entity != null) 'entity': entity!.toJson(),
  };

  /// The tab to open first: flow, diagram, entity, chart or web.
  final String suggested;
  final String? webURL;
  final List<String>? webCandidates;
  final VizSpec? flow;
  final VizSpec? diagram;
  final VizSpec? chart;
  final VizSpec? entity;
}

/// A WEB1 binary frame follows with the same request id; then a second answer, SentResponse.
class WebSnapshotResponse {
  const WebSnapshotResponse({
    this.id,
    this.ok = true,
    required this.data,
  });

  factory WebSnapshotResponse.fromJson(JsonMap json) => WebSnapshotResponse(
    id: json['id'],
    ok: _need(json, 'ok', _asBool),
    data: _need(json, 'data', (Object? v) => WebSnapshotResponseData.fromJson(_asMap(v))),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'ok': ok,
    'data': data.toJson(),
  };

  final Object? id;
  final bool ok;

  /// The answer.
  final WebSnapshotResponseData data;
}

/// The answer.
class WebSnapshotResponseData {
  const WebSnapshotResponseData({
    this.binary = true,
    required this.bytes,
    this.format = 'jpeg',
    required this.url,
  });

  factory WebSnapshotResponseData.fromJson(JsonMap json) => WebSnapshotResponseData(
    binary: _need(json, 'binary', _asBool),
    bytes: _need(json, 'bytes', _asInt),
    format: _need(json, 'format', _asString),
    url: _need(json, 'url', _asString),
  );

  JsonMap toJson() => <String, Object?>{
    'binary': binary,
    'bytes': bytes,
    'format': format,
    'url': url,
  };

  final bool binary;
  final int bytes;
  final String format;
  final String url;
}

/// An IMG1 binary frame follows with the same request id; then a second answer, SentResponse.
class ChatImageResponse {
  const ChatImageResponse({
    this.id,
    this.ok = true,
    required this.data,
  });

  factory ChatImageResponse.fromJson(JsonMap json) => ChatImageResponse(
    id: json['id'],
    ok: _need(json, 'ok', _asBool),
    data: _need(json, 'data', (Object? v) => ChatImageResponseData.fromJson(_asMap(v))),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'ok': ok,
    'data': data.toJson(),
  };

  final Object? id;
  final bool ok;

  /// The answer.
  final ChatImageResponseData data;
}

/// The answer.
class ChatImageResponseData {
  const ChatImageResponseData({
    this.binary = true,
    required this.bytes,
    this.format = 'jpeg',
    required this.imageId,
  });

  factory ChatImageResponseData.fromJson(JsonMap json) => ChatImageResponseData(
    binary: _need(json, 'binary', _asBool),
    bytes: _need(json, 'bytes', _asInt),
    format: _need(json, 'format', _asString),
    imageId: _need(json, 'imageId', _asString),
  );

  JsonMap toJson() => <String, Object?>{
    'binary': binary,
    'bytes': bytes,
    'format': format,
    'imageId': imageId,
  };

  final bool binary;
  final int bytes;
  final String format;
  final String imageId;
}

/// Done.
class SentResponse {
  const SentResponse({
    this.id,
    this.ok = true,
    required this.data,
  });

  factory SentResponse.fromJson(JsonMap json) => SentResponse(
    id: json['id'],
    ok: _need(json, 'ok', _asBool),
    data: _need(json, 'data', (Object? v) => SentResponseData.fromJson(_asMap(v))),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'ok': ok,
    'data': data.toJson(),
  };

  final Object? id;
  final bool ok;

  /// The answer.
  final SentResponseData data;
}

/// The answer.
class SentResponseData {
  const SentResponseData({
    this.sent = true,
  });

  factory SentResponseData.fromJson(JsonMap json) => SentResponseData(
    sent: _need(json, 'sent', _asBool),
  );

  JsonMap toJson() => <String, Object?>{
    'sent': sent,
  };

  final bool sent;
}

/// Done.
typedef SessionMessageResponse = SentResponse;

/// Done.
typedef SessionSendNowResponse = SentResponse;

/// Done.
typedef SessionImageEndResponse = SentResponse;

/// The upload id and the piece size to use.
class SessionImageBeginResponse {
  const SessionImageBeginResponse({
    this.id,
    this.ok = true,
    required this.data,
  });

  factory SessionImageBeginResponse.fromJson(JsonMap json) => SessionImageBeginResponse(
    id: json['id'],
    ok: _need(json, 'ok', _asBool),
    data: _need(json, 'data', (Object? v) => SessionImageBeginResponseData.fromJson(_asMap(v))),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'ok': ok,
    'data': data.toJson(),
  };

  final Object? id;
  final bool ok;

  /// The answer.
  final SessionImageBeginResponseData data;
}

/// The answer.
class SessionImageBeginResponseData {
  const SessionImageBeginResponseData({
    required this.uploadId,
    required this.chunkBytes,
  });

  factory SessionImageBeginResponseData.fromJson(JsonMap json) => SessionImageBeginResponseData(
    uploadId: _need(json, 'uploadId', _asString),
    chunkBytes: _need(json, 'chunkBytes', _asInt),
  );

  JsonMap toJson() => <String, Object?>{
    'uploadId': uploadId,
    'chunkBytes': chunkBytes,
  };

  final String uploadId;

  /// Send pieces of at most this many bytes.
  final int chunkBytes;
}

/// The piece was stored.
class SessionImageChunkResponse {
  const SessionImageChunkResponse({
    this.id,
    this.ok = true,
    required this.data,
  });

  factory SessionImageChunkResponse.fromJson(JsonMap json) => SessionImageChunkResponse(
    id: json['id'],
    ok: _need(json, 'ok', _asBool),
    data: _need(json, 'data', (Object? v) => SessionImageChunkResponseData.fromJson(_asMap(v))),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'ok': ok,
    'data': data.toJson(),
  };

  final Object? id;
  final bool ok;

  /// The answer.
  final SessionImageChunkResponseData data;
}

/// The answer.
class SessionImageChunkResponseData {
  const SessionImageChunkResponseData({
    required this.received,
  });

  factory SessionImageChunkResponseData.fromJson(JsonMap json) => SessionImageChunkResponseData(
    received: _need(json, 'received', _asInt),
  );

  JsonMap toJson() => <String, Object?>{
    'received': received,
  };

  /// The sequence number.
  final int received;
}

/// The slash commands.
class CommandsListResponse {
  const CommandsListResponse({
    this.id,
    this.ok = true,
    required this.data,
  });

  factory CommandsListResponse.fromJson(JsonMap json) => CommandsListResponse(
    id: json['id'],
    ok: _need(json, 'ok', _asBool),
    data: _need(json, 'data', (Object? v) => CommandsListResponseData.fromJson(_asMap(v))),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'ok': ok,
    'data': data.toJson(),
  };

  final Object? id;
  final bool ok;

  /// The answer.
  final CommandsListResponseData data;
}

/// The answer.
class CommandsListResponseData {
  const CommandsListResponseData({
    required this.commands,
  });

  factory CommandsListResponseData.fromJson(JsonMap json) => CommandsListResponseData(
    commands: _need(json, 'commands', (Object? v) => _asList<Command>(v, (Object? e) => Command.fromJson(_asMap(e)))),
  );

  JsonMap toJson() => <String, Object?>{
    'commands': <Object?>[for (final Command e in commands) e.toJson()],
  };

  final List<Command> commands;
}

/// The prompt was answered.
class PromptAnswerResponse {
  const PromptAnswerResponse({
    this.id,
    this.ok = true,
    required this.data,
  });

  factory PromptAnswerResponse.fromJson(JsonMap json) => PromptAnswerResponse(
    id: json['id'],
    ok: _need(json, 'ok', _asBool),
    data: _need(json, 'data', (Object? v) => PromptAnswerResponseData.fromJson(_asMap(v))),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'ok': ok,
    'data': data.toJson(),
  };

  final Object? id;
  final bool ok;

  /// The answer.
  final PromptAnswerResponseData data;
}

/// The answer.
class PromptAnswerResponseData {
  const PromptAnswerResponseData({
    this.answered = true,
  });

  factory PromptAnswerResponseData.fromJson(JsonMap json) => PromptAnswerResponseData(
    answered: _need(json, 'answered', _asBool),
  );

  JsonMap toJson() => <String, Object?>{
    'answered': answered,
  };

  final bool answered;
}

/// The session is starting.
class SessionNewResponse {
  const SessionNewResponse({
    this.id,
    this.ok = true,
    required this.data,
  });

  factory SessionNewResponse.fromJson(JsonMap json) => SessionNewResponse(
    id: json['id'],
    ok: _need(json, 'ok', _asBool),
    data: _need(json, 'data', (Object? v) => SessionNewResponseData.fromJson(_asMap(v))),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'ok': ok,
    'data': data.toJson(),
  };

  final Object? id;
  final bool ok;

  /// The answer.
  final SessionNewResponseData data;
}

/// The answer.
class SessionNewResponseData {
  const SessionNewResponseData({
    this.started = true,
  });

  factory SessionNewResponseData.fromJson(JsonMap json) => SessionNewResponseData(
    started: _need(json, 'started', _asBool),
  );

  JsonMap toJson() => <String, Object?>{
    'started': started,
  };

  final bool started;
}

/// The session to switch to.
class SessionTakeoverResponse {
  const SessionTakeoverResponse({
    this.id,
    this.ok = true,
    required this.data,
  });

  factory SessionTakeoverResponse.fromJson(JsonMap json) => SessionTakeoverResponse(
    id: json['id'],
    ok: _need(json, 'ok', _asBool),
    data: _need(json, 'data', (Object? v) => SessionTakeoverResponseData.fromJson(_asMap(v))),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'ok': ok,
    'data': data.toJson(),
  };

  final Object? id;
  final bool ok;

  /// The answer.
  final SessionTakeoverResponseData data;
}

/// The answer.
class SessionTakeoverResponseData {
  const SessionTakeoverResponseData({
    required this.sessionId,
    this.resumed = true,
    this.stoppedPid,
  });

  factory SessionTakeoverResponseData.fromJson(JsonMap json) => SessionTakeoverResponseData(
    sessionId: _need(json, 'sessionId', _asString),
    resumed: _need(json, 'resumed', _asBool),
    stoppedPid: _maybe(json, 'stoppedPid', _asInt),
  );

  JsonMap toJson() => <String, Object?>{
    'sessionId': sessionId,
    'resumed': resumed,
    if (stoppedPid != null) 'stoppedPid': stoppedPid,
  };

  /// The new session's id; a pty: id while it is still starting.
  final String sessionId;
  final bool resumed;

  /// The pid of the original claude process that was stopped.
  final int? stoppedPid;
}

/// Recent folders.
class ProjectsRecentResponse {
  const ProjectsRecentResponse({
    this.id,
    this.ok = true,
    required this.data,
  });

  factory ProjectsRecentResponse.fromJson(JsonMap json) => ProjectsRecentResponse(
    id: json['id'],
    ok: _need(json, 'ok', _asBool),
    data: _need(json, 'data', (Object? v) => ProjectsRecentResponseData.fromJson(_asMap(v))),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'ok': ok,
    'data': data.toJson(),
  };

  final Object? id;
  final bool ok;

  /// The answer.
  final ProjectsRecentResponseData data;
}

/// The answer.
class ProjectsRecentResponseData {
  const ProjectsRecentResponseData({
    required this.projects,
  });

  factory ProjectsRecentResponseData.fromJson(JsonMap json) => ProjectsRecentResponseData(
    projects: _need(json, 'projects', (Object? v) => _asList<String>(v, _asString)),
  );

  JsonMap toJson() => <String, Object?>{
    'projects': projects,
  };

  final List<String> projects;
}

/// Subscribed.
class SubscribeResponse {
  const SubscribeResponse({
    this.id,
    this.ok = true,
    required this.data,
  });

  factory SubscribeResponse.fromJson(JsonMap json) => SubscribeResponse(
    id: json['id'],
    ok: _need(json, 'ok', _asBool),
    data: _need(json, 'data', (Object? v) => SubscribeResponseData.fromJson(_asMap(v))),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'ok': ok,
    'data': data.toJson(),
  };

  final Object? id;
  final bool ok;

  /// The answer.
  final SubscribeResponseData data;
}

/// The answer.
class SubscribeResponseData {
  const SubscribeResponseData({
    this.subscribed = true,
  });

  factory SubscribeResponseData.fromJson(JsonMap json) => SubscribeResponseData(
    subscribed: _need(json, 'subscribed', _asBool),
  );

  JsonMap toJson() => <String, Object?>{
    'subscribed': subscribed,
  };

  final bool subscribed;
}

/// Unsubscribed.
class UnsubscribeResponse {
  const UnsubscribeResponse({
    this.id,
    this.ok = true,
    required this.data,
  });

  factory UnsubscribeResponse.fromJson(JsonMap json) => UnsubscribeResponse(
    id: json['id'],
    ok: _need(json, 'ok', _asBool),
    data: _need(json, 'data', (Object? v) => UnsubscribeResponseData.fromJson(_asMap(v))),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'ok': ok,
    'data': data.toJson(),
  };

  final Object? id;
  final bool ok;

  /// The answer.
  final UnsubscribeResponseData data;
}

/// The answer.
class UnsubscribeResponseData {
  const UnsubscribeResponseData({
    this.subscribed = false,
  });

  factory UnsubscribeResponseData.fromJson(JsonMap json) => UnsubscribeResponseData(
    subscribed: _need(json, 'subscribed', _asBool),
  );

  JsonMap toJson() => <String, Object?>{
    'subscribed': subscribed,
  };

  final bool subscribed;
}

/// The inbox as the Mac keeps it.
class InboxListResponse {
  const InboxListResponse({
    this.id,
    this.ok = true,
    required this.data,
  });

  factory InboxListResponse.fromJson(JsonMap json) => InboxListResponse(
    id: json['id'],
    ok: _need(json, 'ok', _asBool),
    data: _need(json, 'data', (Object? v) => InboxListResponseData.fromJson(_asMap(v))),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'ok': ok,
    'data': data.toJson(),
  };

  final Object? id;
  final bool ok;

  /// The answer.
  final InboxListResponseData data;
}

/// The answer.
class InboxListResponseData {
  const InboxListResponseData({
    required this.items,
    required this.unread,
  });

  factory InboxListResponseData.fromJson(JsonMap json) => InboxListResponseData(
    items: _need(json, 'items', (Object? v) => _asList<InboxItem>(v, (Object? e) => InboxItem.fromJson(_asMap(e)))),
    unread: _need(json, 'unread', _asInt),
  );

  JsonMap toJson() => <String, Object?>{
    'items': <Object?>[for (final InboxItem e in items) e.toJson()],
    'unread': unread,
  };

  final List<InboxItem> items;

  /// Open items not seen yet.
  final int unread;
}

/// Whether this answer was the one taken.
class InboxAnswerResponse {
  const InboxAnswerResponse({
    this.id,
    this.ok = true,
    required this.data,
  });

  factory InboxAnswerResponse.fromJson(JsonMap json) => InboxAnswerResponse(
    id: json['id'],
    ok: _need(json, 'ok', _asBool),
    data: _need(json, 'data', (Object? v) => InboxAnswerResponseData.fromJson(_asMap(v))),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'ok': ok,
    'data': data.toJson(),
  };

  final Object? id;
  final bool ok;

  /// The answer.
  final InboxAnswerResponseData data;
}

/// The answer.
class InboxAnswerResponseData {
  const InboxAnswerResponseData({
    required this.answered,
    required this.state,
    this.answer,
  });

  factory InboxAnswerResponseData.fromJson(JsonMap json) => InboxAnswerResponseData(
    answered: _need(json, 'answered', _asBool),
    state: _need(json, 'state', _asString),
    answer: _maybe(json, 'answer', _asString),
  );

  JsonMap toJson() => <String, Object?>{
    'answered': answered,
    'state': state,
    if (answer != null) 'answer': answer,
  };

  /// False when the item was already answered, expired or withdrawn.
  final bool answered;
  final String state;

  /// The action that answered the item, when one did.
  final String? answer;
}

/// The Mac refused a request, or could not read the frame (then there is no id).
class ErrorResponse {
  const ErrorResponse({
    this.id,
    this.ok = false,
    required this.error,
  });

  factory ErrorResponse.fromJson(JsonMap json) => ErrorResponse(
    id: json['id'],
    ok: _need(json, 'ok', _asBool),
    error: _need(json, 'error', (Object? v) => ErrorResponseError.fromJson(_asMap(v))),
  );

  JsonMap toJson() => <String, Object?>{
    if (id != null) 'id': id,
    'ok': ok,
    'error': error.toJson(),
  };

  final Object? id;
  final bool ok;

  /// What went wrong.
  final ErrorResponseError error;
}

/// What went wrong.
class ErrorResponseError {
  const ErrorResponseError({
    required this.code,
    required this.message,
  });

  factory ErrorResponseError.fromJson(JsonMap json) => ErrorResponseError(
    code: _need(json, 'code', (Object? v) => ErrorCode.parse(_asString(v))),
    message: _need(json, 'message', _asString),
  );

  JsonMap toJson() => <String, Object?>{
    'code': code.wire,
    'message': message,
  };

  final ErrorCode code;

  /// Text for the person, in English. Never parse it.
  final String message;
}

/// The session list changed (also sent once on subscribe).
class SessionsEvent {
  const SessionsEvent({
    this.event = 'sessions',
    required this.data,
  });

  factory SessionsEvent.fromJson(JsonMap json) => SessionsEvent(
    event: _need(json, 'event', _asString),
    data: _need(json, 'data', (Object? v) => _asList<Session>(v, (Object? e) => Session.fromJson(_asMap(e)))),
  );

  JsonMap toJson() => <String, Object?>{
    'event': event,
    'data': <Object?>[for (final Session e in data) e.toJson()],
  };

  final String event;
  final List<Session> data;
}

/// A new question or permission.
class PromptEvent {
  const PromptEvent({
    this.event = 'prompt',
    required this.sessionId,
    required this.prompt,
  });

  factory PromptEvent.fromJson(JsonMap json) => PromptEvent(
    event: _need(json, 'event', _asString),
    sessionId: _need(json, 'sessionId', _asString),
    prompt: _need(json, 'prompt', (Object? v) => Prompt.fromJson(_asMap(v))),
  );

  JsonMap toJson() => <String, Object?>{
    'event': event,
    'sessionId': sessionId,
    'prompt': prompt.toJson(),
  };

  final String event;
  final String sessionId;
  final Prompt prompt;
}

/// A prompt was answered elsewhere or timed out.
class PromptResolvedEvent {
  const PromptResolvedEvent({
    this.event = 'prompt',
    required this.requestId,
    this.resolved = true,
  });

  factory PromptResolvedEvent.fromJson(JsonMap json) => PromptResolvedEvent(
    event: _need(json, 'event', _asString),
    requestId: _need(json, 'requestId', _asString),
    resolved: _need(json, 'resolved', _asBool),
  );

  JsonMap toJson() => <String, Object?>{
    'event': event,
    'requestId': requestId,
    'resolved': resolved,
  };

  final String event;
  final String requestId;
  final bool resolved;
}

/// New messages in the subscribed session.
class ChatEvent {
  const ChatEvent({
    this.event = 'chat',
    required this.sessionId,
    required this.messages,
  });

  factory ChatEvent.fromJson(JsonMap json) => ChatEvent(
    event: _need(json, 'event', _asString),
    sessionId: _need(json, 'sessionId', _asString),
    messages: _need(json, 'messages', (Object? v) => _asList<ChatMessage>(v, (Object? e) => ChatMessage.fromJson(_asMap(e)))),
  );

  JsonMap toJson() => <String, Object?>{
    'event': event,
    'sessionId': sessionId,
    'messages': <Object?>[for (final ChatMessage e in messages) e.toJson()],
  };

  final String event;
  final String sessionId;
  final List<ChatMessage> messages;
}

/// The person chose Open on the Mac: the phone should come to the front.
class OpenEvent {
  const OpenEvent({
    this.event = 'open',
  });

  factory OpenEvent.fromJson(JsonMap json) => OpenEvent(
    event: _need(json, 'event', _asString),
  );

  JsonMap toJson() => <String, Object?>{
    'event': event,
  };

  final String event;
}

/// An inbox item was raised or changed. Sent only to a connection that subscribed with inbox: true.
class InboxEvent {
  const InboxEvent({
    this.event = 'inbox',
    required this.item,
  });

  factory InboxEvent.fromJson(JsonMap json) => InboxEvent(
    event: _need(json, 'event', _asString),
    item: _need(json, 'item', (Object? v) => InboxItem.fromJson(_asMap(v))),
  );

  JsonMap toJson() => <String, Object?>{
    'event': event,
    'item': item.toJson(),
  };

  final String event;
  final InboxItem item;
}

/// An inbox item was removed from the Mac's inbox.
class InboxRemovedEvent {
  const InboxRemovedEvent({
    this.event = 'inbox',
    required this.removed,
  });

  factory InboxRemovedEvent.fromJson(JsonMap json) => InboxRemovedEvent(
    event: _need(json, 'event', _asString),
    removed: _need(json, 'removed', _asString),
  );

  JsonMap toJson() => <String, Object?>{
    'event': event,
    'removed': removed,
  };

  final String event;

  /// The removed item's id.
  final String removed;
}

/// Parses a map as the type named by the key and writes it back.
final Map<String, JsonMap Function(JsonMap)> roundtrips = <String, JsonMap Function(JsonMap)>{
  'Subagent': (JsonMap json) => Subagent.fromJson(json).toJson(),
  'PromptQuestion': (JsonMap json) => PromptQuestion.fromJson(json).toJson(),
  'Prompt': (JsonMap json) => Prompt.fromJson(json).toJson(),
  'Takeover': (JsonMap json) => Takeover.fromJson(json).toJson(),
  'LastMessage': (JsonMap json) => LastMessage.fromJson(json).toJson(),
  'Limit': (JsonMap json) => Limit.fromJson(json).toJson(),
  'Facts': (JsonMap json) => Facts.fromJson(json).toJson(),
  'Session': (JsonMap json) => Session.fromJson(json).toJson(),
  'Image': (JsonMap json) => Image.fromJson(json).toJson(),
  'ToolStep': (JsonMap json) => ToolStep.fromJson(json).toJson(),
  'ChatMessage': (JsonMap json) => ChatMessage.fromJson(json).toJson(),
  'Command': (JsonMap json) => Command.fromJson(json).toJson(),
  'VizMessage': (JsonMap json) => VizMessage.fromJson(json).toJson(),
  'VizFailure': (JsonMap json) => VizFailure.fromJson(json).toJson(),
  'FlowCall': (JsonMap json) => FlowCall.fromJson(json).toJson(),
  'FlowTurn': (JsonMap json) => FlowTurn.fromJson(json).toJson(),
  'FlowFile': (JsonMap json) => FlowFile.fromJson(json).toJson(),
  'FlowSpec': (JsonMap json) => FlowSpec.fromJson(json).toJson(),
  'DiagramBlock': (JsonMap json) => DiagramBlock.fromJson(json).toJson(),
  'DiagramSpec': (JsonMap json) => DiagramSpec.fromJson(json).toJson(),
  'EntityFile': (JsonMap json) => EntityFile.fromJson(json).toJson(),
  'EntityTable': (JsonMap json) => EntityTable.fromJson(json).toJson(),
  'EntityLink': (JsonMap json) => EntityLink.fromJson(json).toJson(),
  'EntitySpec': (JsonMap json) => EntitySpec.fromJson(json).toJson(),
  'UsagePoint': (JsonMap json) => UsagePoint.fromJson(json).toJson(),
  'UsageTotals': (JsonMap json) => UsageTotals.fromJson(json).toJson(),
  'ChartCount': (JsonMap json) => ChartCount.fromJson(json).toJson(),
  'ModelCount': (JsonMap json) => ModelCount.fromJson(json).toJson(),
  'ChartSpec': (JsonMap json) => ChartSpec.fromJson(json).toJson(),
  'VizSpec': (JsonMap json) => VizSpec.fromJson(json).toJson(),
  'InboxAction': (JsonMap json) => InboxAction.fromJson(json).toJson(),
  'InboxItem': (JsonMap json) => InboxItem.fromJson(json).toJson(),
  'HelloRequest': (JsonMap json) => HelloRequest.fromJson(json).toJson(),
  'PairRequest': (JsonMap json) => PairRequest.fromJson(json).toJson(),
  'PingRequest': (JsonMap json) => PingRequest.fromJson(json).toJson(),
  'SessionsListRequest': (JsonMap json) => SessionsListRequest.fromJson(json).toJson(),
  'ChatGetRequest': (JsonMap json) => ChatGetRequest.fromJson(json).toJson(),
  'VizGetRequest': (JsonMap json) => VizGetRequest.fromJson(json).toJson(),
  'WebSnapshotRequest': (JsonMap json) => WebSnapshotRequest.fromJson(json).toJson(),
  'SessionMessageRequest': (JsonMap json) => SessionMessageRequest.fromJson(json).toJson(),
  'SessionSendNowRequest': (JsonMap json) => SessionSendNowRequest.fromJson(json).toJson(),
  'SessionImageBeginRequest': (JsonMap json) => SessionImageBeginRequest.fromJson(json).toJson(),
  'SessionImageChunkRequest': (JsonMap json) => SessionImageChunkRequest.fromJson(json).toJson(),
  'SessionImageEndRequest': (JsonMap json) => SessionImageEndRequest.fromJson(json).toJson(),
  'CommandsListRequest': (JsonMap json) => CommandsListRequest.fromJson(json).toJson(),
  'PromptAnswerRequest': (JsonMap json) => PromptAnswerRequest.fromJson(json).toJson(),
  'SessionNewRequest': (JsonMap json) => SessionNewRequest.fromJson(json).toJson(),
  'ChatImageRequest': (JsonMap json) => ChatImageRequest.fromJson(json).toJson(),
  'SessionTakeoverRequest': (JsonMap json) => SessionTakeoverRequest.fromJson(json).toJson(),
  'ProjectsRecentRequest': (JsonMap json) => ProjectsRecentRequest.fromJson(json).toJson(),
  'SubscribeRequest': (JsonMap json) => SubscribeRequest.fromJson(json).toJson(),
  'UnsubscribeRequest': (JsonMap json) => UnsubscribeRequest.fromJson(json).toJson(),
  'InboxListRequest': (JsonMap json) => InboxListRequest.fromJson(json).toJson(),
  'InboxAnswerRequest': (JsonMap json) => InboxAnswerRequest.fromJson(json).toJson(),
  'HelloResponse': (JsonMap json) => HelloResponse.fromJson(json).toJson(),
  'PairResponse': (JsonMap json) => PairResponse.fromJson(json).toJson(),
  'PingResponse': (JsonMap json) => PingResponse.fromJson(json).toJson(),
  'SessionsListResponse': (JsonMap json) => SessionsListResponse.fromJson(json).toJson(),
  'ChatGetResponse': (JsonMap json) => ChatGetResponse.fromJson(json).toJson(),
  'VizGetResponse': (JsonMap json) => VizGetResponse.fromJson(json).toJson(),
  'WebSnapshotResponse': (JsonMap json) => WebSnapshotResponse.fromJson(json).toJson(),
  'ChatImageResponse': (JsonMap json) => ChatImageResponse.fromJson(json).toJson(),
  'SentResponse': (JsonMap json) => SentResponse.fromJson(json).toJson(),
  'SessionMessageResponse': (JsonMap json) => SessionMessageResponse.fromJson(json).toJson(),
  'SessionSendNowResponse': (JsonMap json) => SessionSendNowResponse.fromJson(json).toJson(),
  'SessionImageEndResponse': (JsonMap json) => SessionImageEndResponse.fromJson(json).toJson(),
  'SessionImageBeginResponse': (JsonMap json) => SessionImageBeginResponse.fromJson(json).toJson(),
  'SessionImageChunkResponse': (JsonMap json) => SessionImageChunkResponse.fromJson(json).toJson(),
  'CommandsListResponse': (JsonMap json) => CommandsListResponse.fromJson(json).toJson(),
  'PromptAnswerResponse': (JsonMap json) => PromptAnswerResponse.fromJson(json).toJson(),
  'SessionNewResponse': (JsonMap json) => SessionNewResponse.fromJson(json).toJson(),
  'SessionTakeoverResponse': (JsonMap json) => SessionTakeoverResponse.fromJson(json).toJson(),
  'ProjectsRecentResponse': (JsonMap json) => ProjectsRecentResponse.fromJson(json).toJson(),
  'SubscribeResponse': (JsonMap json) => SubscribeResponse.fromJson(json).toJson(),
  'UnsubscribeResponse': (JsonMap json) => UnsubscribeResponse.fromJson(json).toJson(),
  'InboxListResponse': (JsonMap json) => InboxListResponse.fromJson(json).toJson(),
  'InboxAnswerResponse': (JsonMap json) => InboxAnswerResponse.fromJson(json).toJson(),
  'ErrorResponse': (JsonMap json) => ErrorResponse.fromJson(json).toJson(),
  'SessionsEvent': (JsonMap json) => SessionsEvent.fromJson(json).toJson(),
  'PromptEvent': (JsonMap json) => PromptEvent.fromJson(json).toJson(),
  'PromptResolvedEvent': (JsonMap json) => PromptResolvedEvent.fromJson(json).toJson(),
  'ChatEvent': (JsonMap json) => ChatEvent.fromJson(json).toJson(),
  'OpenEvent': (JsonMap json) => OpenEvent.fromJson(json).toJson(),
  'InboxEvent': (JsonMap json) => InboxEvent.fromJson(json).toJson(),
  'InboxRemovedEvent': (JsonMap json) => InboxRemovedEvent.fromJson(json).toJson(),
};
