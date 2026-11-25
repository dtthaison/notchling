import 'package:flutter/material.dart';

import '../../core/protocol/models.dart';
import '../../l10n/app_localizations.dart';

enum TakeoverChoice { cancel, resume, stopAndResume }

bool canTakeOver(Session session) => session.control == SessionControl.viewOnly && (session.takeover?.available == true || session.takeover?.stoppable == true);

Future<TakeoverChoice> confirmTakeover(BuildContext context, Session session) async {
  final AppL10n l10n = AppL10n.of(context);
  final TakeoverInfo? info = session.takeover;
  final bool stop = info?.stoppable == true;

  final bool? first = await showDialog<bool>(
    context: context,
    builder: (BuildContext context) => AlertDialog(
      title: Text(l10n.chatTakeoverTitle),
      content: Text(stop ? l10n.chatTakeoverStopBody : l10n.chatTakeoverBody),
      actions: <Widget>[
        TextButton(onPressed: () => Navigator.pop(context, false), child: Text(l10n.appCancel)),
        TextButton(onPressed: () => Navigator.pop(context, true), child: Text(stop ? l10n.consentStopAndTakeOver : l10n.appTakeover)),
      ],
    ),
  );
  if (first != true || !context.mounted) return TakeoverChoice.cancel;
  if (!stop) return TakeoverChoice.resume;
  if (info?.busy == true) {
    final bool? second = await showDialog<bool>(
      context: context,
      builder: (BuildContext context) => AlertDialog(
        title: Text(l10n.chatTakeoverBusyTitle),
        content: Text(l10n.chatTakeoverBusyBody),
        actions: <Widget>[
          TextButton(onPressed: () => Navigator.pop(context, false), child: Text(l10n.appCancel)),
          TextButton(onPressed: () => Navigator.pop(context, true), child: Text(l10n.consentStopAnyway)),
        ],
      ),
    );
    if (second != true) return TakeoverChoice.cancel;
  }
  return TakeoverChoice.stopAndResume;
}
