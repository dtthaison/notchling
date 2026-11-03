import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/tokens.dart';
import '../../app/widgets/common.dart';
import '../../core/protocol/json.dart';
import '../../l10n/app_localizations.dart';
import '../sessions/sessions_notifier.dart';
import 'phone_tab.dart';
import 'viz_providers.dart';
import 'viz_webview.dart';
import 'web_tab.dart';

/// The Visualize tab: Phone, Web, Flow, Diagram, Entity, Chart for the selected session. The Mac
/// suggests one; a tab the user picks is kept for that session.
class VisualizeScreen extends ConsumerWidget {
  const VisualizeScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final AppL10n l10n = AppL10n.of(context);
    final String? id = ref.watch(selectedSessionIdProvider);
    if (id == null) return StateMessage(icon: Icons.insights, title: l10n.vizSelectSession);

    final AsyncValue<Json> viz = ref.watch(vizProvider(id));
    final VizTab? suggested = VizTab.byName(viz.value?.strOrNull('suggested'));
    final VizTab selected = ref.watch(vizTabChoiceProvider)[id] ?? suggested ?? VizTab.flow;

    return Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: <Widget>[
      _TabBar(
        selected: selected,
        suggested: suggested,
        onSelect: (VizTab tab) => ref.read(vizTabChoiceProvider.notifier).choose(id, tab),
      ),
      const Divider(),
      Expanded(
        child: switch (selected) {
          VizTab.phone => const PhoneTab(),
          VizTab.web => WebTab(key: ValueKey<String>('web-$id-${viz.value?.strOrNull('webURL')}'), sessionId: id, initialUrl: viz.value?.strOrNull('webURL')),
          VizTab.entity => _EntityBody(sessionId: id),
          _ => _SpecBody(viz: viz, tab: selected, onRetry: () => ref.invalidate(vizProvider(id))),
        },
      ),
    ]);
  }
}

class _TabBar extends StatelessWidget {
  const _TabBar({required this.selected, required this.suggested, required this.onSelect});
  final VizTab selected;
  final VizTab? suggested;
  final ValueChanged<VizTab> onSelect;

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    return SingleChildScrollView(
      scrollDirection: Axis.horizontal,
      padding: const EdgeInsets.symmetric(horizontal: Space.sm, vertical: Space.xs),
      child: Row(children: <Widget>[
        for (final VizTab tab in VizTab.values)
          Semantics(
            button: true,
            selected: tab == selected,
            label: tab == suggested ? '${tab.title}, ${l10n.vizSuggested}' : tab.title,
            excludeSemantics: true,
            onTap: () => onSelect(tab),
            child: InkWell(
              borderRadius: BorderRadius.circular(Radii.sm),
              onTap: () => onSelect(tab),
              child: Container(
                constraints: const BoxConstraints(minHeight: Metric.minTarget, minWidth: Metric.minTarget),
                padding: const EdgeInsets.symmetric(horizontal: Space.md),
                margin: const EdgeInsets.only(right: Space.xs),
                alignment: Alignment.center,
                decoration: BoxDecoration(color: tab == selected ? Token.surfaceRaised : Colors.transparent, borderRadius: BorderRadius.circular(Radii.sm)),
                child: Row(mainAxisSize: MainAxisSize.min, children: <Widget>[
                  Text(tab.title, style: TypeToken.labelSm.copyWith(color: tab == selected ? Token.primary : Token.muted, fontSize: 13)),
                  if (tab == suggested) ...<Widget>[
                    const SizedBox(width: 4),
                    Container(width: 5, height: 5, decoration: const BoxDecoration(color: Token.muted, shape: BoxShape.circle)),
                  ],
                ]),
              ),
            ),
          ),
      ]),
    );
  }
}

class _SpecBody extends StatelessWidget {
  const _SpecBody({required this.viz, required this.tab, required this.onRetry});
  final AsyncValue<Json> viz;
  final VizTab tab;
  final VoidCallback onRetry;

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final Json? data = viz.value;
    if (data != null) {
      final Object? spec = data[tab.name];
      if (spec is Map<String, Object?>) return VizWebView(spec: spec);
    }
    if (viz.hasError && data == null) return ErrorState(message: l10n.vizLoadFailed, onRetry: onRetry);
    return const SkeletonRows();
  }
}

class _EntityBody extends ConsumerWidget {
  const _EntityBody({required this.sessionId});
  final String sessionId;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final AppL10n l10n = AppL10n.of(context);
    final AsyncValue<Json> entity = ref.watch(vizEntityProvider(sessionId));
    final Object? spec = entity.value?['entity'];
    if (spec is Map<String, Object?>) return VizWebView(spec: spec);
    if (entity.hasError) return ErrorState(message: l10n.vizLoadFailed, onRetry: () => ref.invalidate(vizEntityProvider(sessionId)));
    return const SkeletonRows();
  }
}
