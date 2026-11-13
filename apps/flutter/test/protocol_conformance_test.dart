import 'dart:convert';
import 'dart:io';

import 'package:agent_console/core/protocol/generated.dart' as wire;
import 'package:agent_console/core/protocol/models.dart';
import 'package:flutter_test/flutter_test.dart';

typedef Json = Map<String, Object?>;

final Directory fixturesDir = Directory('../../protocol/fixtures');
final File schemaFile = File('../../protocol/agentconsole.v1.schema.json');

Json asMap(Object? value) => (value! as Map<Object?, Object?>).cast<String, Object?>();
List<Object?> asList(Object? value) => value! as List<Object?>;
Json readJson(File file) => asMap(jsonDecode(file.readAsStringSync()));

List<(String file, String name, Json json)> fixtures() {
  final List<File> files = fixturesDir.listSync().whereType<File>().where((File f) => f.path.endsWith('.json')).toList()
    ..sort((File a, File b) => a.path.compareTo(b.path));
  return <(String, String, Json)>[
    for (final File file in files)
      (file.uri.pathSegments.last, file.uri.pathSegments.last.split('.').first, readJson(file)),
  ];
}

Map<String, Object?> schemaDefaults(Object? node, [Map<String, Object?>? into]) {
  final Map<String, Object?> found = into ?? <String, Object?>{};
  if (node is Map<Object?, Object?>) {
    final Object? properties = node['properties'];
    if (properties is Map<Object?, Object?>) {
      for (final MapEntry<Object?, Object?> entry in properties.entries) {
        final Object? property = entry.value;
        if (property is Map<Object?, Object?> && property.containsKey('default')) found['${entry.key}'] = property['default'];
      }
    }
    for (final Object? child in node.values) {
      schemaDefaults(child, found);
    }
  } else if (node is List<Object?>) {
    for (final Object? child in node) {
      schemaDefaults(child, found);
    }
  }
  return found;
}

void compare(Object? input, Object? output, Map<String, Object?> defaults, String path, List<String> problems) {
  if (input is Map<Object?, Object?> && output is Map<Object?, Object?>) {
    input.forEach((Object? key, Object? value) {
      if (!output.containsKey(key)) {
        problems.add('$path.$key was dropped');
      } else {
        compare(value, output[key], defaults, '$path.$key', problems);
      }
    });
    output.forEach((Object? key, Object? value) {
      if (input.containsKey(key)) return;
      final Object? expected = defaults['$key'];
      final bool isDefault = defaults.containsKey('$key') && jsonEncode(expected) == jsonEncode(value);
      if (!isDefault) problems.add('$path.$key = $value appeared and is not a schema default');
    });
  } else if (input is List<Object?> && output is List<Object?>) {
    if (input.length != output.length) {
      problems.add('$path has ${input.length} items, wrote back ${output.length}');
      return;
    }
    for (int i = 0; i < input.length; i++) {
      compare(input[i], output[i], defaults, '$path[$i]', problems);
    }
  } else if (input is num && output is num) {
    if (input != output) problems.add('$path: $input became $output');
  } else if (input != output) {
    problems.add('$path: $input became $output');
  }
}

void main() {
  test('the fixtures and the schema are where the test expects them', () {
    expect(fixturesDir.existsSync(), isTrue, reason: 'run flutter test from apps/flutter');
    expect(fixtures().length, greaterThan(60));
  });

  test('every fixture parses with the generated types and writes back the same JSON', () {
    final Map<String, Object?> defaults = schemaDefaults(readJson(schemaFile));
    expect(defaults, isNotEmpty);
    for (final (String file, String name, Json json) in fixtures()) {
      final Json Function(Json)? roundtrip = wire.roundtrips[name];
      expect(roundtrip, isNotNull, reason: '$file: generated.dart has no type named $name');
      final Json output = roundtrip!(json);
      final List<String> problems = <String>[];
      compare(json, output, defaults, r'$', problems);
      expect(problems, isEmpty, reason: file);
      expect(jsonEncode(roundtrip(output)), jsonEncode(output), reason: '$file: a second round trip differs');
    }
  });

  test('the hand-written models read what the generated types read', () {
    int checked = 0;
    for (final (String file, String name, Json json) in fixtures()) {
      switch (name) {
        case 'SessionsListResponse':
          _sameSessions(file, asList(asMap(json['data'])['sessions']), asMap(json['data'])['sessions']! as List<Object?>);
          checked++;
        case 'SessionsEvent':
          _sameSessions(file, asList(json['data']), asList(json['data']));
          checked++;
        case 'ChatGetResponse':
        case 'ChatEvent':
          final List<Object?> raw = asList(name == 'ChatEvent' ? json['messages'] : asMap(json['data'])['messages']);
          final List<ChatMessage> messages = raw.map((Object? m) => ChatMessage.fromJson(asMap(m))).toList();
          expect(messages.length, raw.length, reason: file);
          for (int i = 0; i < raw.length; i++) {
            final Json m = asMap(raw[i]);
            expect(messages[i].id, m['id'], reason: file);
            expect(messages[i].text, m['text'], reason: file);
            expect(messages[i].role.name, m['role'], reason: file);
            expect(messages[i].queued, (m['queued'] as bool?) ?? false, reason: file);
            expect(messages[i].steps.length, (m['steps'] as List<Object?>?)?.length ?? 0, reason: file);
            final wire.ChatMessage generated = wire.ChatMessage.fromJson(m);
            expect(generated.id, messages[i].id, reason: file);
            expect(generated.images.length, messages[i].images.length, reason: file);
            expect(generated.queued, messages[i].queued, reason: file);
          }
          checked++;
        case 'PromptEvent':
          if (json['resolved'] != true) {
            final PendingPrompt prompt = PendingPrompt.fromJson(asMap(json['prompt']));
            final wire.PromptEvent generated = wire.PromptEvent.fromJson(json);
            expect(generated.prompt.requestId, prompt.requestId, reason: file);
            expect(generated.prompt.questions.length, prompt.questions.length, reason: file);
            checked++;
          }
      }
    }
    expect(checked, greaterThanOrEqualTo(8), reason: 'the models should have read the session and chat fixtures');
  });

  test('a required field that is missing is an error', () {
    final Json ping = readJson(File('${fixturesDir.path}/PingResponse.json'))..remove('ok');
    expect(() => wire.PingResponse.fromJson(ping), throwsFormatException);
  });

  test('a value the schema does not know reads as unknown', () {
    final Json busy = readJson(File('${fixturesDir.path}/ErrorResponse.busy.json'));
    asMap(busy['error'])['code'] = 'a_code_from_the_future';
    expect(wire.ErrorResponse.fromJson(busy).error.code, wire.ErrorCode.unknown);
    expect(wire.ErrorCode.parse('view_only'), wire.ErrorCode.viewOnly);
  });

  test('an older Mac without the additive session fields still parses', () {
    final Json list = readJson(File('${fixturesDir.path}/SessionsListResponse.json'));
    final Json first = asMap(asList(asMap(list['data'])['sessions']).first);
    for (final String key in <String>['tool', 'subagents', 'facts', 'activity', 'lastMessage', 'pending', 'takeover']) {
      first.remove(key);
    }
    final wire.Session generated = wire.Session.fromJson(first);
    expect(generated.tool, 'claude');
    expect(generated.subagents, isEmpty);
    expect(generated.facts, isNull);
    final Session model = Session.fromJson(first);
    expect(model.subagents, isEmpty);
    expect(model.facts, isNull);
  });
}

void _sameSessions(String file, List<Object?> raw, List<Object?> again) {
  expect(raw, isNotEmpty, reason: file);
  for (final Object? item in raw) {
    final Json json = asMap(item);
    final Session model = Session.fromJson(json);
    final wire.Session generated = wire.Session.fromJson(json);
    expect(model.id, generated.id, reason: file);
    expect(model.name, generated.name, reason: file);
    expect(model.cwd, generated.cwd, reason: file);
    expect(model.entrypoint, generated.entrypoint, reason: file);
    expect(model.state.name, generated.state.wire, reason: file);
    expect(model.control.name, generated.control.wire, reason: file);
    expect(model.lastActivity.millisecondsSinceEpoch, generated.lastActivity.round(), reason: file);
    expect(model.subagents.length, generated.subagents.length, reason: file);
    expect(model.pending?.requestId, generated.pending?.requestId, reason: file);
    expect(model.takeover?.available, generated.takeover?.available, reason: file);
    expect(model.takeover?.stoppable ?? false, generated.takeover?.stoppable ?? false, reason: file);
    expect(model.facts?.contextTokens, generated.facts?.contextTokens, reason: file);
    expect(model.facts?.limits.length, generated.facts?.limits.length, reason: file);
    expect(model.lastMessage?.id, generated.lastMessage?.id, reason: file);
    expect(model.activity, generated.activity, reason: file);
  }
  expect(again.length, raw.length, reason: file);
}
