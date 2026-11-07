import 'json.dart';

enum SessionState {
  needsInput,
  running,
  done,
  exited;

  static SessionState parse(String raw) => switch (raw) {
    'needsInput' => SessionState.needsInput,
    'running' => SessionState.running,
    'exited' => SessionState.exited,
    _ => SessionState.done,
  };
}

/// How the Mac can reach a session: its own pty, a background job, or not at all.
enum SessionControl {
  console,
  job,
  viewOnly;

  static SessionControl parse(String raw) => switch (raw) {
    'console' => SessionControl.console,
    'job' => SessionControl.job,
    _ => SessionControl.viewOnly,
  };
}

DateTime _fromMillis(double ms) =>
    DateTime.fromMillisecondsSinceEpoch(ms.round());

class Subagent {
  const Subagent({
    required this.id,
    required this.type,
    required this.description,
    required this.model,
    required this.finished,
    required this.running,
    required this.lastActivity,
  });

  factory Subagent.fromJson(Json json) => Subagent(
    id: json.str('id'),
    type: json.str('type'),
    description: json.str('description'),
    model: json.str('model'),
    finished: json.boolean('finished'),
    running: json.str('state') == 'running',
    lastActivity: _fromMillis(json.number('lastActivity')),
  );

  final String id;
  final String type;
  final String description;
  final String model;
  final bool finished;
  final bool running;
  final DateTime lastActivity;
}

class PromptQuestion {
  const PromptQuestion({
    required this.header,
    required this.question,
    required this.options,
    required this.multiSelect,
  });

  factory PromptQuestion.fromJson(Json json) => PromptQuestion(
    header: json.strOrNull('header') ?? '',
    question: json.str('question'),
    options: json.strings('options'),
    multiSelect: json.boolean('multiSelect'),
  );

  final String header;
  final String question;
  final List<String> options;
  final bool multiSelect;
}

/// A question or permission a hook on the Mac is waiting on.
class PendingPrompt {
  const PendingPrompt({
    required this.requestId,
    required this.sessionId,
    required this.isPermission,
    required this.toolName,
    required this.summary,
    required this.questions,
  });

  factory PendingPrompt.fromJson(Json json) => PendingPrompt(
    requestId: json.str('requestId'),
    sessionId: json.str('sessionId'),
    isPermission: json.str('kind') == 'permission',
    toolName: json.strOrNull('toolName') ?? '',
    summary: json.strOrNull('summary') ?? '',
    questions: json.objects('questions').map(PromptQuestion.fromJson).toList(),
  );

  final String requestId;
  final String sessionId;
  final bool isPermission;
  final String toolName;
  final String summary;
  final List<PromptQuestion> questions;

  bool get hasMultiSelect => questions.any((PromptQuestion q) => q.multiSelect);
}

/// The Mac's answer to "can you take this session over": [reason] says why not.
class TakeoverInfo {
  const TakeoverInfo({
    required this.available,
    this.reason,
    this.stoppable = false,
    this.busy = false,
  });

  factory TakeoverInfo.fromJson(Json json) => TakeoverInfo(
    available: json['available'] == true,
    reason: json.strOrNull('reason'),
    stoppable: json['stoppable'] == true,
    busy: json['busy'] == true,
  );

  /// Can be resumed right now.
  final bool available;
  final String? reason;

  /// The original is still running but the Mac can stop it and then resume.
  final bool stoppable;

  /// The original is in the middle of a turn, which stopping interrupts.
  final bool busy;
}

/// A cheap marker of Claude's newest reply, to announce it without reading the chat.
class LastMessage {
  const LastMessage({required this.id, required this.text});
  factory LastMessage.fromJson(Json json) => LastMessage(id: json.str('id'), text: json.strOrNull('text') ?? '');
  final String id;
  final String text;
}

/// One limit of the account a session runs on, as a percentage used ("5h", "Week").
class LimitMeter {
  const LimitMeter({required this.label, required this.percent, this.resetsAt});

  factory LimitMeter.fromJson(Json json) => LimitMeter(
    label: json.str('label'),
    percent: json.number('percent'),
    resetsAt: json['resetsAt'] is num
        ? _fromMillis((json['resetsAt']! as num).toDouble())
        : null,
  );

  final String label;
  final double percent;
  final DateTime? resetsAt;
}

/// What the chat header shows: the context in use, the model's window (null when unknown), and the limits.
class SessionFacts {
  const SessionFacts({
    this.contextTokens,
    this.contextWindow,
    this.model,
    this.limits = const <LimitMeter>[],
  });

  factory SessionFacts.fromJson(Json json) => SessionFacts(
    contextTokens: json['contextTokens'] is num
        ? (json['contextTokens']! as num).toInt()
        : null,
    contextWindow: json['contextWindow'] is num
        ? (json['contextWindow']! as num).toInt()
        : null,
    model: json.strOrNull('model'),
    limits: json['limits'] is List<Object?>
        ? json.objects('limits').map(LimitMeter.fromJson).toList()
        : const <LimitMeter>[],
  );

  final int? contextTokens;
  final int? contextWindow;
  final String? model;
  final List<LimitMeter> limits;

  /// 0..1, or null when the window is unknown.
  double? get contextFraction =>
      contextTokens == null || contextWindow == null || contextWindow == 0
      ? null
      : (contextTokens! / contextWindow!).clamp(0, 1).toDouble();
}

class Session {
  const Session({
    required this.id,
    required this.name,
    required this.project,
    required this.cwd,
    required this.entrypoint,
    required this.state,
    required this.control,
    required this.lastActivity,
    this.subagents = const <Subagent>[],
    this.pending,
    this.takeover,
    this.activity,
    this.facts,
    this.lastMessage,
  });

  factory Session.fromJson(Json json) {
    final Object? pending = json['pending'];
    return Session(
      id: json.str('id'),
      name: json.str('name'),
      project: json.str('project'),
      cwd: json.str('cwd'),
      entrypoint: json.str('entrypoint'),
      state: SessionState.parse(json.str('state')),
      control: SessionControl.parse(json.str('control')),
      lastActivity: _fromMillis(json.number('lastActivity')),
      subagents: json.objects('subagents').map(Subagent.fromJson).toList(),
      pending: pending is Map<Object?, Object?>
          ? PendingPrompt.fromJson(pending.cast<String, Object?>())
          : null,
      takeover: json['takeover'] is Map<Object?, Object?>
          ? TakeoverInfo.fromJson(json.object('takeover'))
          : null,
      activity: json.strOrNull('activity'),
      lastMessage: json['lastMessage'] is Map<Object?, Object?> ? LastMessage.fromJson(json.object('lastMessage')) : null,
      facts: json['facts'] is Map<Object?, Object?>
          ? SessionFacts.fromJson(json.object('facts'))
          : null,
    );
  }

  final String id;
  final String name;
  final String project;
  final String cwd;
  final String entrypoint;
  final SessionState state;
  final SessionControl control;
  final DateTime lastActivity;
  final List<Subagent> subagents;
  final PendingPrompt? pending;

  /// For view-only sessions: whether the Mac can take the session over, and why not. Null from older Macs.
  final TakeoverInfo? takeover;

  /// What the session is doing right now ("Editing main.swift"). Null from older Macs.
  final String? activity;

  /// Context size and the account's limits, for the chat header. Null from older Macs.
  final SessionFacts? facts;

  /// Claude's newest reply (id and first lines); a new id means a new reply. Null from older Macs.
  final LastMessage? lastMessage;

  int get runningSubagents => subagents.where((Subagent s) => s.running).length;
  bool get canMessage => control != SessionControl.viewOnly;
}

enum ChatRole {
  user,
  assistant,
  tools;

  static ChatRole parse(String raw) => switch (raw) {
    'user' => ChatRole.user,
    'tools' => ChatRole.tools,
    _ => ChatRole.assistant,
  };
}

/// A picture shown in a message. Only this description travels with the chat; the bytes are fetched on demand.
class ChatImage {
  const ChatImage({
    required this.id,
    required this.mediaType,
    this.width,
    this.height,
    this.bytes = 0,
  });

  factory ChatImage.fromJson(Json json) => ChatImage(
    id: json.str('id'),
    mediaType: json.str('mediaType'),
    width: json['width'] is num ? (json['width']! as num).toInt() : null,
    height: json['height'] is num ? (json['height']! as num).toInt() : null,
    bytes: json['bytes'] is num ? (json['bytes']! as num).toInt() : 0,
  );

  Json toJson() => <String, Object?>{
    'id': id,
    'mediaType': mediaType,
    'width': width,
    'height': height,
    'bytes': bytes,
  };

  final String id;
  final String mediaType;
  final int? width;
  final int? height;
  final int bytes;

  /// Width over height when both are known, so the placeholder has the picture's shape before it loads.
  double? get aspect =>
      width != null && height != null && height! > 0 ? width! / height! : null;
}

class ChatMessage {
  const ChatMessage({
    required this.id,
    required this.role,
    required this.text,
    required this.time,
    this.toolSummary,
    this.queued = false,
    this.images = const <ChatImage>[],
    this.kind = ChatKind.text,
    this.steps = const <ToolStep>[],
    this.output,
  });

  factory ChatMessage.fromJson(Json json) => ChatMessage(
    id: json.str('id'),
    role: ChatRole.parse(json.str('role')),
    text: json.str('text'),
    time: _fromMillis(json.number('time')),
    toolSummary: json.strOrNull('toolSummary'),
    queued: json['queued'] == true,
    images: json['images'] is List<Object?>
        ? json.objects('images').map(ChatImage.fromJson).toList()
        : const <ChatImage>[],
    kind: ChatKind.parse(json.strOrNull('kind')),
    steps: json['steps'] is List<Object?>
        ? json.objects('steps').map(ToolStep.fromJson).toList()
        : const <ToolStep>[],
    output: json.strOrNull('output'),
  );

  Json toJson() => <String, Object?>{
    'id': id,
    'role': role.name,
    'text': text,
    'time': time.millisecondsSinceEpoch,
    'toolSummary': toolSummary,
    'queued': queued,
    'images': images.map((ChatImage i) => i.toJson()).toList(),
    'kind': kind.name,
    'steps': steps.map((ToolStep t) => t.toJson()).toList(),
    'output': output,
  };

  final String id;
  final ChatRole role;
  final String text;
  final DateTime time;
  final String? toolSummary;

  /// Sent while the agent was working and not yet taken by it ("đang chờ").
  final bool queued;

  final List<ChatImage> images;

  /// A normal message, a `!` shell command (text is the command, [output] what it printed), or the notice that the
  /// conversation was compacted (text is the summary line).
  final ChatKind kind;

  /// For a tools message: the calls of one step, with their input summary, output and diff when there is one.
  final List<ToolStep> steps;
  final String? output;
}

enum ChatKind {
  text,
  shell,
  compaction;

  static ChatKind parse(String? raw) => switch (raw) {
    'shell' => ChatKind.shell,
    'compaction' => ChatKind.compaction,
    _ => ChatKind.text,
  };
}

/// One line of a diff: context, added or removed.
enum DiffKind { context, added, removed }

class DiffLine {
  const DiffLine(this.kind, this.text);
  final DiffKind kind;
  final String text;
}

/// One tool call inside a step: its name, a one-line summary, what it printed, and for Edit/Write the diff.
class ToolStep {
  const ToolStep({
    required this.name,
    required this.summary,
    this.output,
    this.path,
    this.diff = const <DiffLine>[],
  });

  factory ToolStep.fromJson(Json json) => ToolStep(
    name: json.str('name'),
    summary: json.str('summary'),
    output: json.strOrNull('output'),
    path: json.strOrNull('path'),
    diff: json['diff'] is List<Object?>
        ? <DiffLine>[
            for (final Object? row in json['diff']! as List<Object?>)
              if (row is String && row.isNotEmpty)
                DiffLine(
                  row[0] == '+'
                      ? DiffKind.added
                      : (row[0] == '-' ? DiffKind.removed : DiffKind.context),
                  row.substring(1),
                ),
          ]
        : const <DiffLine>[],
  );

  /// Diff rows are one string each: "+added", "-removed", " context".
  Json toJson() => <String, Object?>{
    'name': name,
    'summary': summary,
    'output': output,
    'path': path,
    'diff': <String>[
      for (final DiffLine l in diff)
        '${l.kind == DiffKind.added ? '+' : (l.kind == DiffKind.removed ? '-' : ' ')}${l.text}',
    ],
  };

  final String name;
  final String summary;
  final String? output;
  final String? path;
  final List<DiffLine> diff;

  bool get isEdit => diff.isNotEmpty;
}
