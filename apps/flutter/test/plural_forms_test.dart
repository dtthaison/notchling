import 'package:agent_console/l10n/app_localizations_en.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  final AppL10nEn l10n = AppL10nEn();

  test('tool_steps_agree_with_their_count', () {
    expect(l10n.chatToolStepsCount(1), '1 tool step');
    expect(l10n.chatToolStepsCount(0), '0 tool steps');
    expect(l10n.chatToolStepsCount(4), '4 tool steps');
  });

  test('subagent_words_agree_with_their_count', () {
    expect(l10n.sessionsRunningSubagents(1), '1 subagent running');
    expect(l10n.sessionsRunningSubagents(2), '2 subagents running');
    expect(l10n.sessionsSubagentsShow(1), 'Show 1 subagent');
    expect(l10n.sessionsSubagentsShow(3), 'Show 3 subagents');
    expect(l10n.sessionsSubagentsHide(1), 'Hide 1 subagent');
    expect(l10n.sessionsSubagentsHide(3), 'Hide 3 subagents');
  });

  test('accessibility_labels_agree_with_their_count', () {
    expect(l10n.sessionsGroupLabel(1, 'Working'), 'Working, 1 session');
    expect(l10n.sessionsGroupLabel(2, 'Working'), 'Working, 2 sessions');
    expect(l10n.macsComputerHeadingLabel(1, 'Studio', 'Connected'), 'Studio, Connected, 1 session');
    expect(l10n.macsComputerHeadingLabel(5, 'Studio', 'Connected'), 'Studio, Connected, 5 sessions');
  });

  test('watching_sessions_agree_with_their_count', () {
    expect(l10n.notificationsWatchingSessions(1), 'Watching · 1 session');
    expect(l10n.notificationsWatchingSessions(2), 'Watching · 2 sessions');
  });
}
