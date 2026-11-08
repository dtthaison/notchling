import 'dart:async';
import 'dart:io' show Platform;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/alerts.dart';
import '../../app/theme/tokens.dart';
import '../../app/widgets/common.dart';
import '../../core/pairing/mac_profile.dart';
import '../../l10n/app_localizations.dart';
import '../connection/connection.dart';
import '../pairing/pairing_screen.dart';
import 'computer_heading.dart';
import 'macs_notifier.dart';

/// The paired Macs: switch the active one, add another, remove one.
class MacsScreen extends ConsumerWidget {
  const MacsScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final AppL10n l10n = AppL10n.of(context);
    final MacsState state = ref.watch(macsProvider);
    return Scaffold(
      appBar: AppBar(backgroundColor: Kit.of(context).sidebar, title: Text(l10n.macsTitle, style: Kit.of(context).titleMd)),
      body: SafeArea(
        child: ListView(
          padding: const EdgeInsets.all(Space.lg),
          children: <Widget>[
            if (Platform.isAndroid) ...<Widget>[
              SwitchListTile(
                contentPadding: EdgeInsets.zero,
                title: Text(l10n.keepAliveSetting, style: Kit.of(context).bodyMd),
                subtitle: Text(l10n.keepAliveHint, style: Kit.of(context).bodySm.copyWith(color: Kit.of(context).textMuted)),
                value: ref.watch(alertPrefsProvider.select((AlertPrefs p) => p.keepAlive == true)),
                onChanged: (bool on) => unawaited(ref.read(alertPrefsProvider.notifier).setKeepAlive(on: on)),
              ),
              const Divider(),
            ],
            for (final MacProfile mac in state.macs)
              Container(
                margin: const EdgeInsets.only(bottom: Space.md),
                padding: const EdgeInsets.all(Space.md),
                decoration: BoxDecoration(
                  color: Kit.of(context).sidebar,
                  borderRadius: BorderRadius.circular(Radii.md),
                  border: Border.all(color: mac.id == state.activeId ? Kit.of(context).accent : Kit.of(context).hairline),
                ),
                child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: <Widget>[
                  Row(children: <Widget>[
                    Icon(Icons.laptop_mac, size: 18, color: Kit.of(context).textMuted),
                    const SizedBox(width: Space.sm),
                    Expanded(child: Text(mac.name, style: Kit.of(context).titleMd)),
                    if (mac.id == state.activeId) Text(l10n.macsActive, style: Kit.of(context).labelSm.copyWith(color: Kit.of(context).accent)),
                  ]),
                  const SizedBox(height: Space.xs),
                  Text('${mac.lastGoodHost ?? mac.hosts.first}:${mac.port}', style: Kit.of(context).codeSm),
                  const SizedBox(height: Space.xs),
                  // Each computer has its own token and connection; this is its own link, not the one in use.
                  ComputerLinkLabel(status: ref.watch(computerStatusProvider(mac.id))),
                  const SizedBox(height: Space.md),
                  Wrap(spacing: Space.sm, runSpacing: Space.sm, children: <Widget>[
                    if (mac.id != state.activeId) SecondaryButton(label: l10n.macsUse, onPressed: () => unawaited(ref.read(macsProvider.notifier).setActive(mac.id))),
                    SecondaryButton(label: l10n.macsRemove, onPressed: () => unawaited(_confirmRemove(context, ref, mac))),
                  ]),
                ]),
              ),
            SecondaryButton(
              label: l10n.macsAdd,
              icon: Icons.add,
              onPressed: () => unawaited(Navigator.of(context).push(MaterialPageRoute<void>(builder: (BuildContext context) => const PairingScreen(canCancel: true)))),
            ),
          ],
        ),
      ),
    );
  }

  Future<void> _confirmRemove(BuildContext context, WidgetRef ref, MacProfile mac) async {
    final AppL10n l10n = AppL10n.of(context);
    final bool? ok = await showDialog<bool>(
      context: context,
      builder: (BuildContext context) => AlertDialog(
        title: Text(l10n.macsRemove),
        content: Text(l10n.macsRemoveConfirm(mac.name)),
        actions: <Widget>[
          TextButton(onPressed: () => Navigator.pop(context, false), child: Text(l10n.cancel)),
          TextButton(onPressed: () => Navigator.pop(context, true), child: Text(l10n.macsRemove)),
        ],
      ),
    );
    if (ok == true) await ref.read(macsProvider.notifier).remove(mac.id);
  }
}
