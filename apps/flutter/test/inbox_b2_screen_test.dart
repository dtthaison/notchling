import 'package:agent_console/app/widgets/common.dart';
import 'package:agent_console/features/inbox/inbox_notifier.dart';
import 'package:agent_console/features/inbox/inbox_screen.dart';
import 'package:agent_console/core/protocol/models.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/harness.dart';

class _Fixed extends InboxNotifier {
  _Fixed(this.fixed);
  final InboxState fixed;
  @override
  InboxState build() => fixed;
}

InboxItem make(String id, Map<String, Object?> extra) => InboxItem.fromJson(<String, Object?>{
      'id': id,
      'kind': 'consent',
      'title': 'Title $id',
      'detail': '',
      'createdMs': 1000,
      'actions': <Object?>[
        <String, Object?>{'id': 'allow', 'label': 'Allow', 'primary': true},
        <String, Object?>{'id': 'deny', 'label': 'Deny', 'primary': false},
        <String, Object?>{'id': 'change', 'label': 'Change…', 'primary': false},
      ],
      'state': 'open',
      'seen': false,
      ...extra,
    });

Future<void> show(WidgetTester tester, List<InboxItem> items) async {
  await tester.pumpWidget(ProviderScope(
    overrides: [inboxProvider.overrideWith(() => _Fixed(InboxState(phase: InboxPhase.ready, items: items)))],
    child: host(const Scaffold(body: InboxScreen()), locale: const Locale('en'), screen: true),
  ));
  await tester.pump();
}

void main() {
  testWidgets('Blocking sits above Needs You, with the wait, the place and no Change', (WidgetTester tester) async {
    final int now = DateTime.now().millisecondsSinceEpoch;
    await show(tester, <InboxItem>[
      make('rest', <String, Object?>{'createdMs': now}),
      make('wait', <String, Object?>{
        'blocking': true,
        'waitingSince': now - 190000,
        'place': <String, Object?>{'machineName': 'build', 'sessionName': 'fix login', 'branch': 'main'},
      }),
    ]);
    expect(find.text('WAITING ON YOU'), findsOneWidget);
    expect(find.text('NEEDS YOU'), findsOneWidget);
    expect(tester.getTopLeft(find.text('WAITING ON YOU')).dy, lessThan(tester.getTopLeft(find.text('NEEDS YOU')).dy));
    expect(find.text('Stopped 3 minutes ago'), findsOneWidget);
    expect(find.text('build'), findsOneWidget);
    expect(find.text('fix login'), findsOneWidget);
    expect(find.text('main'), findsOneWidget);
    expect(find.text('Change…'), findsNothing);
    expect(find.widgetWithText(PrimaryButton, 'Allow'), findsOneWidget);
  });

  testWidgets('an older Mac sends no Blocking section', (WidgetTester tester) async {
    await show(tester, <InboxItem>[make('a', <String, Object?>{})]);
    expect(find.text('WAITING ON YOU'), findsNothing);
    expect(find.text('NEEDS YOU'), findsOneWidget);
  });

  testWidgets('closed items show the outcome and the note sent back', (WidgetTester tester) async {
    await show(tester, <InboxItem>[
      make('f', <String, Object?>{
        'state': 'answered',
        'answer': 'allow',
        'answeredBy': 'mac',
        'outcome': <String, Object?>{'state': 'failed', 'reason': 'The setting could not be changed.', 'atMs': 5},
      }),
      make('n', <String, Object?>{'state': 'answered', 'answer': 'deny', 'answeredBy': 'mac', 'note': 'use staging'}),
      make('p', <String, Object?>{'state': 'answered', 'answer': 'allow', 'answeredBy': 'mac', 'outcome': <String, Object?>{'state': 'applying', 'atMs': 5}}),
      make('a', <String, Object?>{'state': 'answered', 'answer': 'allow', 'answeredBy': 'mac', 'outcome': <String, Object?>{'state': 'applied', 'atMs': 5}}),
      make('w', <String, Object?>{'state': 'withdrawn', 'outcome': <String, Object?>{'state': 'withdrawn', 'atMs': 5}}),
    ]);
    expect(find.text('Failed: The setting could not be changed.'), findsOneWidget);
    expect(find.text('Sent back: use staging'), findsOneWidget);
    expect(find.text('Applying…'), findsOneWidget);
    expect(find.text('Applied'), findsOneWidget);
    expect(find.text('Withdrawn'), findsOneWidget);
  });
}
