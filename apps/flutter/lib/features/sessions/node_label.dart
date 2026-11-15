import 'package:flutter/material.dart';

import '../../app/theme/tokens.dart';
import '../../core/protocol/models.dart';
import '../../l10n/app_localizations.dart';

const double offlineOpacity = 0.55;

String nodeSpoken(AppL10n l10n, SessionNode node) {
  final String on = l10n.sessionsNodeOn(node.name);
  return node.online ? on : '$on, ${l10n.sessionsNodeOffline}';
}

class NodeLabel extends StatelessWidget {
  const NodeLabel({super.key, required this.node});

  final SessionNode node;

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final Kit kit = Kit.of(context);
    final Widget label = ExcludeSemantics(
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: <Widget>[
          Icon(Icons.dns_outlined, size: 13, color: kit.textMuted),
          const SizedBox(width: 4),
          Flexible(
            flex: 2,
            child: Text(
              node.name,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: kit.meta,
            ),
          ),
          if (!node.online) ...<Widget>[
            const SizedBox(width: 6),
            Flexible(
              child: Text(
                l10n.sessionsNodeOffline,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: kit.meta.copyWith(color: kit.textFaint),
              ),
            ),
          ],
        ],
      ),
    );
    return node.online ? label : Opacity(opacity: offlineOpacity, child: label);
  }
}

class NodeLabelSlot extends StatelessWidget {
  const NodeLabelSlot({super.key, required this.node});

  final SessionNode node;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(right: Space.sm),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 160),
        child: NodeLabel(node: node),
      ),
    );
  }
}
