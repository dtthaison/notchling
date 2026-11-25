import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/chat/chat_header.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/fake_ws.dart';

Session session({String state = 'running', Map<String, Object?>? facts, String activity = 'Reading'}) =>
    Session.fromJson(<String, Object?>{...sessionJson('s', state: state), 'activity': activity, 'facts': ?facts});

void main() {
  group('what the chat header watches', () {
    test('a push of the same facts is the same value although the Session is a new object', () {
      final Map<String, Object?> facts = <String, Object?>{
        'contextTokens': 1000,
        'contextWindow': 4000,
        'model': 'm',
        'limits': <Object?>[<String, Object?>{'label': '5h', 'percent': 10.0}],
      };
      final Session a = session(facts: facts);
      final Session b = session(facts: facts);
      expect(identical(a, b), isFalse);
      expect(headerShown(a), headerShown(b));
    });

    test('a change the header draws is a different value', () {
      final Map<String, Object?> facts = <String, Object?>{'contextTokens': 1000, 'contextWindow': 4000};
      final Object? base = headerShown(session(facts: facts));
      expect(headerShown(session(state: 'done', facts: facts)), isNot(base));
      expect(headerShown(session(facts: facts, activity: 'Editing')), isNot(base));
      expect(headerShown(session(facts: <String, Object?>{'contextTokens': 2000, 'contextWindow': 4000})), isNot(base));
      expect(
        headerShown(session(facts: <String, Object?>{...facts, 'limits': <Object?>[<String, Object?>{'label': '5h', 'percent': 11.0}]})),
        isNot(base),
      );
    });

    test('no session is no value', () {
      expect(headerShown(null), isNull);
    });
  });
}
