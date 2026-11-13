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
      title: Text(l10n.takeoverTitle),
      content: Text(stop ? l10n.takeoverStopBody : l10n.takeoverBody),
      actions: <Widget>[
        TextButton(onPressed: () => Navigator.pop(context, false), child: Text(l10n.cancel)),
        TextButton(onPressed: () => Navigator.pop(context, true), child: Text(stop ? l10n.takeoverStopConfirm : l10n.takeover)),
      ],
    ),
  );
  if (first != true || !context.mounted) return TakeoverChoice.cancel;
  if (!stop) return TakeoverChoice.resume;
  if (info?.busy == true) {
    final bool? second = await showDialog<bool>(
      context: context,
      builder: (BuildContext context) => AlertDialog(
        title: Text(l10n.takeoverBusyTitle),
        content: Text(l10n.takeoverBusyBody),
        actions: <Widget>[
          TextButton(onPressed: () => Navigator.pop(context, false), child: Text(l10n.cancel)),
          TextButton(onPressed: () => Navigator.pop(context, true), child: Text(l10n.takeoverBusyConfirm)),
        ],
      ),
    );
    if (second != true) return TakeoverChoice.cancel;
  }
  return TakeoverChoice.stopAndResume;
}
