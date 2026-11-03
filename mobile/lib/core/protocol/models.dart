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

DateTime _fromMillis(double ms) => DateTime.fromMillisecondsSinceEpoch(ms.round());

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
  const PromptQuestion({required this.header, required this.question, required this.options, required this.multiSelect});

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
  const TakeoverInfo({required this.available, this.reason, this.stoppable = false, this.busy = false});

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
      pending: pending is Map<Object?, Object?> ? PendingPrompt.fromJson(pending.cast<String, Object?>()) : null,
      takeover: json['takeover'] is Map<Object?, Object?> ? TakeoverInfo.fromJson(json.object('takeover')) : null,
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
  const ChatImage({required this.id, required this.mediaType, this.width, this.height, this.bytes = 0});

  factory ChatImage.fromJson(Json json) => ChatImage(
        id: json.str('id'),
        mediaType: json.str('mediaType'),
        width: json['width'] is num ? (json['width']! as num).toInt() : null,
        height: json['height'] is num ? (json['height']! as num).toInt() : null,
        bytes: json['bytes'] is num ? (json['bytes']! as num).toInt() : 0,
      );

  Json toJson() => <String, Object?>{'id': id, 'mediaType': mediaType, 'width': width, 'height': height, 'bytes': bytes};

  final String id;
  final String mediaType;
  final int? width;
  final int? height;
  final int bytes;

  /// Width over height when both are known, so the placeholder has the picture's shape before it loads.
  double? get aspect => width != null && height != null && height! > 0 ? width! / height! : null;
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
  });

  factory ChatMessage.fromJson(Json json) => ChatMessage(
        id: json.str('id'),
        role: ChatRole.parse(json.str('role')),
        text: json.str('text'),
        time: _fromMillis(json.number('time')),
        toolSummary: json.strOrNull('toolSummary'),
        queued: json['queued'] == true,
        images: json['images'] is List<Object?> ? json.objects('images').map(ChatImage.fromJson).toList() : const <ChatImage>[],
      );

  Json toJson() => <String, Object?>{
        'id': id,
        'role': role.name,
        'text': text,
        'time': time.millisecondsSinceEpoch,
        'toolSummary': toolSummary,
        'queued': queued,
        'images': images.map((ChatImage i) => i.toJson()).toList(),
      };

  final String id;
  final ChatRole role;
  final String text;
  final DateTime time;
  final String? toolSummary;

  /// Sent while the agent was working and not yet taken by it ("đang chờ").
  final bool queued;

  final List<ChatImage> images;
}
