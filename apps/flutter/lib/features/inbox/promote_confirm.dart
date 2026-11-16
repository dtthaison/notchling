import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/protocol/models.dart';
import '../../l10n/app_localizations.dart';
import '../machines/confirm_dialog.dart';
import '../sessions/sessions_notifier.dart';

const Set<String> movingActions = <String>{'move_here', 'move_anyway', 'take_back', 'continue_here'};

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
      title = l10n.inboxConfirmContinueTitle(name);
      body = l10n.inboxConfirmContinueBodyUnnamed;
  }
  return askConfirm(context, title: title, body: body, action: action.label);
}
