import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/protocol/models.dart';
import '../../l10n/app_localizations.dart';
import '../sessions/sessions_notifier.dart';

const Set<String> movingActions = <String>{'move_here', 'move_anyway', 'take_back', 'continue_here'};

const String _waitLabelPrefix = 'Wait for ';

String? _otherMachine(InboxItem item) {
  for (final InboxAction action in item.actions) {
    if (action.id == 'wait_for' && action.label.startsWith(_waitLabelPrefix)) {
      final String name = action.label.substring(_waitLabelPrefix.length).trim();
      if (name.isNotEmpty) return name;
    }
  }
  return null;
}

Future<bool> confirmInboxAction(BuildContext context, WidgetRef ref, InboxItem item, InboxAction action) async {
  if (!movingActions.contains(action.id)) return true;
  final AppL10n l10n = AppL10n.of(context);
  final String title;
  final String body;
  switch (action.id) {
    case 'move_here':
      title = l10n.inboxConfirmMoveHereTitle;
      body = l10n.inboxConfirmMoveHereBody;
    case 'move_anyway':
      title = l10n.inboxConfirmMoveAnywayTitle;
      body = item.detail;
    case 'take_back':
      title = l10n.inboxConfirmTakeBackTitle;
      body = l10n.inboxConfirmTakeBackBody;
    default:
      final String name = ref.read(sessionsProvider).byId(item.session)?.name ?? item.title;
      final String? machine = _otherMachine(item);
      title = l10n.inboxConfirmContinueTitle(name);
      body = machine == null ? l10n.inboxConfirmContinueBodyUnnamed : l10n.inboxConfirmContinueBody(machine);
  }
  final bool? yes = await showDialog<bool>(
    context: context,
    builder: (BuildContext context) => AlertDialog(
      title: Text(title),
      content: body.isEmpty ? null : Text(body),
      actions: <Widget>[
        TextButton(onPressed: () => Navigator.pop(context, false), child: Text(l10n.appCancel)),
        TextButton(onPressed: () => Navigator.pop(context, true), child: Text(action.label)),
      ],
    ),
  );
  return yes == true;
}
