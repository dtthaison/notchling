import 'package:flutter/material.dart';

import '../../l10n/app_localizations.dart';

Future<bool> askConfirm(BuildContext context, {required String title, required String body, required String action}) async {
  final AppL10n l10n = AppL10n.of(context);
  bool closed = false;
  final bool? yes = await showDialog<bool>(
    context: context,
    builder: (BuildContext dialog) {
      void close(bool answer) {
        if (closed) return;
        closed = true;
        Navigator.pop(dialog, answer);
      }

      return AlertDialog(
        title: Text(title),
        content: body.isEmpty ? null : Text(body),
        actions: <Widget>[
          TextButton(onPressed: () => close(false), child: Text(l10n.appCancel)),
          TextButton(onPressed: () => close(true), child: Text(action)),
        ],
      );
    },
  );
  return yes == true;
}
