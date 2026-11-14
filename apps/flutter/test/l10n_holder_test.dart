import 'dart:ui';

import 'package:agent_console/app/l10n_holder.dart';
import 'package:agent_console/l10n/app_localizations.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  setUp(() => L10nHolder.update(const <Locale>[Locale('en')]));

  test('holder_before_first_frame_is_english', () {
    expect(L10nHolder.current.appName, 'Agent Console');
    expect(L10nHolder.current.localeName, 'en');
  });

  test('update_with_unknown_locale_stays_english', () {
    L10nHolder.update(const <Locale>[Locale('de'), Locale('ja', 'JP')]);
    expect(L10nHolder.current.localeName, 'en');
    L10nHolder.update(null);
    expect(L10nHolder.current.localeName, 'en');
    L10nHolder.update(const <Locale>[]);
    expect(L10nHolder.current.localeName, 'en');
  });

  test('update_with_english_variant_picks_english', () {
    L10nHolder.update(const <Locale>[Locale('en', 'GB')]);
    expect(L10nHolder.current.localeName, 'en');
    expect(L10nHolder.current.appRetry, 'Retry');
  });

  test('the_foreground_text_counts_sessions', () {
    expect(L10nHolder.current.notificationsWatching, 'Watching');
    expect(L10nHolder.current.notificationsWatchingSessions(1), 'Watching · 1 session');
    expect(L10nHolder.current.notificationsWatchingSessions(3), 'Watching · 3 sessions');
  });

  test('notification_words_are_english', () {
    final AppL10n l10n = L10nHolder.current;
    for (final String text in <String>[
      l10n.notificationsChannelNeedsYou,
      l10n.notificationsChannelFinished,
      l10n.notificationsChannelMessages,
      l10n.notificationsSummaryBody,
      l10n.notificationsNeedsBody,
      l10n.notificationsFinishedBody,
    ]) {
      expect(text, isNotEmpty);
      expect(RegExp(r'^[\x20-\x7e]+$').hasMatch(text), isTrue, reason: text);
    }
  });
}
