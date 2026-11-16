import '../../core/protocol/json.dart';

class PromoteMachine {
  const PromoteMachine({
    required this.id,
    required this.name,
    required this.online,
    required this.canBeMaster,
    required this.ready,
    required this.canResume,
    this.setAtMs,
  });

  factory PromoteMachine.fromJson(Json json) => PromoteMachine(
    id: json.str('id'),
    name: json.strOrNull('name') ?? json.str('id'),
    online: json.boolean('online'),
    canBeMaster: json.boolean('canBeMaster'),
    ready: json.boolean('ready', orElse: true),
    canResume: json.boolean('canResume'),
    setAtMs: json['setAt'] is num ? json.integer('setAt') : null,
  );

  final String id;
  final String name;
  final bool online;
  final bool canBeMaster;
  final bool ready;
  final bool canResume;
  final int? setAtMs;

  DateTime? get setAt => setAtMs == null ? null : DateTime.fromMillisecondsSinceEpoch(setAtMs!);

  bool get pickable => canBeMaster && ready;
}

class PromoteAnswer {
  const PromoteAnswer({this.step, this.relayed = false, this.text, this.needsConfirm});

  factory PromoteAnswer.fromJson(Json json) => PromoteAnswer(
    step: json.strOrNull('step'),
    relayed: json.boolean('relayed'),
    text: json.strOrNull('text'),
    needsConfirm: json.strOrNull('needsConfirm'),
  );

  final String? step;
  final bool relayed;
  final String? text;
  final String? needsConfirm;
}

String clockText(DateTime time) {
  String two(int n) => n.toString().padLeft(2, '0');
  return '${two(time.hour)}:${two(time.minute)}';
}
