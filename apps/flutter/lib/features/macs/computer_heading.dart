import 'package:flutter/material.dart';

import '../../app/theme/tokens.dart';
import '../../core/net/bridge_client.dart';
import '../../l10n/app_localizations.dart';

class ComputerHeading {
  const ComputerHeading({required this.id, required this.name, required this.status});

  final String id;
  final String name;
  final ConnectionStatus status;
}

class ComputerHeader extends StatelessWidget {
  const ComputerHeader({super.key, required this.computer, required this.count});
  final ComputerHeading computer;
  final int count;

  @override
  Widget build(BuildContext context) {
    final Kit kit = Kit.of(context);
    return Semantics(
      header: true,
      label: '${computer.name}, ${linkWord(AppL10n.of(context), computer.status)}, $count session',
      excludeSemantics: true,
      child: Padding(
        padding: const EdgeInsets.fromLTRB(Space.lg, Space.xl, Space.lg, 0),
        child: Row(
          children: <Widget>[
            Icon(Icons.computer, size: 18, color: kit.textMuted),
            const SizedBox(width: Space.sm),
            Expanded(child: Text(computer.name, style: kit.heading, maxLines: 1, overflow: TextOverflow.ellipsis)),
            const SizedBox(width: Space.sm),
            ComputerLinkLabel(status: computer.status),
          ],
        ),
      ),
    );
  }
}

String linkWord(AppL10n l10n, ConnectionStatus status) => switch (status.phase) {
      LinkPhase.connected => l10n.connected,
      LinkPhase.connecting => l10n.connecting,
      LinkPhase.disconnected when status.revoked => l10n.computerRevoked,
      LinkPhase.disconnected => l10n.computerOffline,
    };

class ComputerLinkLabel extends StatelessWidget {
  const ComputerLinkLabel({super.key, required this.status});
  final ConnectionStatus status;

  @override
  Widget build(BuildContext context) {
    final Kit kit = Kit.of(context);
    final bool down = status.phase == LinkPhase.disconnected;
    final Color color = down ? kit.danger : kit.textMuted;
    final IconData icon = switch (status.phase) {
      LinkPhase.connected => Icons.link,
      LinkPhase.connecting => Icons.sync,
      LinkPhase.disconnected => status.revoked ? Icons.link_off : Icons.wifi_off,
    };
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: <Widget>[
        Icon(icon, size: 14, color: color),
        const SizedBox(width: Space.xs),
        Text(linkWord(AppL10n.of(context), status), style: kit.meta.copyWith(color: color)),
      ],
    );
  }
}
