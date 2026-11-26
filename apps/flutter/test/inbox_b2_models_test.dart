import 'package:agent_console/core/protocol/models.dart';
import 'package:flutter_test/flutter_test.dart';

Map<String, Object?> base() => <String, Object?>{
      'id': 'consent-9',
      'kind': 'consent',
      'title': 'Claude Code asks to change a setting',
      'detail': '',
      'createdMs': 1793541465005,
      'actions': <Object?>[
        <String, Object?>{'id': 'allow', 'label': 'Allow', 'primary': true},
        <String, Object?>{'id': 'deny', 'label': 'Deny', 'primary': false},
      ],
      'state': 'open',
      'seen': false,
    };

void main() {
  test('an item with every B2 field reads them all', () {
    final InboxItem item = InboxItem.fromJson(<String, Object?>{
      ...base(),
      'expiresMs': 1793541645005,
      'closedMs': 1793541465009,
      'blocking': true,
      'waitingSince': 1793541465005,
      'waiter': 'cli',
      'alertActions': <Object?>['allow', 'deny', 'change'],
      'note': 'use the staging value',
      'outcome': <String, Object?>{'state': 'failed', 'reason': 'The setting could not be changed.', 'atMs': 1793541465009},
      'place': <String, Object?>{
        'machine': '0123456789abcdef',
        'machineName': 'build',
        'session': '5b6d9a30',
        'sessionName': 'fix login',
        'branch': 'main',
      },
    });
    expect(item.blocking, isTrue);
    expect(item.isBlocking, isTrue);
    expect(item.waitingSince, 1793541465005);
    expect(item.waiter, 'cli');
    expect(item.expiresMs, 1793541645005);
    expect(item.note, 'use the staging value');
    expect(item.outcome?.state, 'failed');
    expect(item.outcome?.reason, 'The setting could not be changed.');
    expect(item.outcome?.atMs, 1793541465009);
    expect(item.place?.machineName, 'build');
    expect(item.place?.sessionName, 'fix login');
    expect(item.place?.branch, 'main');
    expect(item.place?.labels, <String>['build', 'fix login', 'main']);
  });

  test('an item from an older Mac with none of them still reads', () {
    final InboxItem item = InboxItem.fromJson(base());
    expect(item.blocking, isNull);
    expect(item.isBlocking, isFalse);
    expect(item.waitingSince, isNull);
    expect(item.waiter, isNull);
    expect(item.expiresMs, isNull);
    expect(item.note, isNull);
    expect(item.outcome, isNull);
    expect(item.place, isNull);
  });

  test('a closed item keeps the B2 fields', () {
    final InboxItem open = InboxItem.fromJson(<String, Object?>{
      ...base(),
      'blocking': true,
      'waitingSince': 5,
      'note': 'n',
      'place': <String, Object?>{'machineName': 'build'},
    });
    final InboxItem closed = open.closed(state: 'answered', answer: 'deny', answeredBy: 'phone');
    expect(closed.isBlocking, isFalse);
    expect(closed.blocking, isTrue);
    expect(closed.note, 'n');
    expect(closed.place?.machineName, 'build');
  });

  test('a place with no names has no labels', () {
    expect(InboxItem.fromJson(<String, Object?>{...base(), 'place': <String, Object?>{}}).place?.labels, isEmpty);
  });
}
