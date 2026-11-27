import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../features/macs/macs_notifier.dart';
import '../../features/sessions/sessions_notifier.dart';
import '../message_alerts.dart';
import '../nav_state.dart';
import '../alerts.dart';
import '../theme/tokens.dart';
import 'kit.dart';
import 'motion.dart';

class InAppAlertBanner extends ConsumerWidget {
  const InAppAlertBanner({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final AlertEvent? event = ref.watch(inAppAlertProvider);
    return AlertBannerView(
      event: event,
      onTap: () {
        if (event == null) return;
        ref.read(selectedSessionProvider.notifier).select(event.session.onHostIfUnknown(ref.read(macsProvider).active?.id ?? ''));
        ref.read(activeTabProvider.notifier).show(1);
        ref.read(inAppAlertProvider.notifier).dismiss();
      },
      onDismiss: () => ref.read(inAppAlertProvider.notifier).dismiss(),
    );
  }
}

class AlertBannerView extends StatelessWidget {
  const AlertBannerView({super.key, required this.event, required this.onTap, required this.onDismiss});
  final AlertEvent? event;
  final VoidCallback onTap;
  final VoidCallback onDismiss;

  @override
  Widget build(BuildContext context) {
    final Kit kit = Kit.of(context);
    final AlertEvent? e = event;
    return AnimatedSwitcher(
      duration: motionDuration(context, const Duration(milliseconds: 420)),
      reverseDuration: motionDuration(context, Motion.ui),
      switchInCurve: Curves.easeOutBack,
      switchOutCurve: Curves.easeIn,
      transitionBuilder: (Widget child, Animation<double> a) => SlideTransition(
        position: Tween<Offset>(begin: const Offset(0, -1.4), end: Offset.zero).animate(a),
        child: FadeTransition(opacity: a, child: child),
      ),
      layoutBuilder: (Widget? current, List<Widget> previous) => Stack(alignment: Alignment.topCenter, children: <Widget>[...previous, ?current]),
      child: e == null
          ? const SizedBox.shrink(key: ValueKey<String>('no-alert'))
          : Padding(
              key: ValueKey<String>('alert-${e.session.qualified}'),
              padding: const EdgeInsets.fromLTRB(Space.md, Space.sm, Space.md, 0),
              child: Semantics(
                container: true,
                liveRegion: true,
                button: true,
                label: '${e.name}: ${e.body}',
                excludeSemantics: true,
                onTap: onTap,
                child: GestureDetector(
                  behavior: HitTestBehavior.opaque,
                  onTap: onTap,
                  onVerticalDragEnd: (DragEndDetails d) {
                    if ((d.primaryVelocity ?? 0) < -150) {
                      unawaited(HapticFeedback.selectionClick());
                      onDismiss();
                    }
                  },
                  child: Surface(
                    level: 2,
                    radius: BorderRadius.circular(Radii.md),
                    padding: const EdgeInsets.all(Space.md),
                    child: Row(
                      crossAxisAlignment: CrossAxisAlignment.center,
                      children: <Widget>[
                        KitAvatar(name: e.name, size: 36),
                        const SizedBox(width: Space.md),
                        Expanded(
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            mainAxisSize: MainAxisSize.min,
                            children: <Widget>[
                              Text(e.name, maxLines: 1, overflow: TextOverflow.ellipsis, style: kit.heading),
                              const SizedBox(height: 1),
                              Text(e.body, maxLines: 2, overflow: TextOverflow.ellipsis, style: kit.meta.copyWith(color: e.kind == AlertKind.needsInput ? kit.accentText : kit.text)),
                            ],
                          ),
                        ),
                        if (e.kind == AlertKind.needsInput) ...<Widget>[
                          const SizedBox(width: Space.sm),
                          Icon(Icons.mark_chat_unread_outlined, size: 20, color: kit.accentText),
                        ],
                      ],
                    ),
                  ),
                ),
              ),
            ),
    );
  }
}
