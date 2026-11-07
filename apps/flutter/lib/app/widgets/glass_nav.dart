import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../../l10n/app_localizations.dart';
import '../theme/tokens.dart';
import 'kit.dart';
import 'motion.dart';

/// The frosted tab bar: three tabs, one pill that slides to the selected one with a spring-like curve, the icon
/// morphing from outline to filled with a small pop, the label getting heavier. Pure: the app wraps it with the
/// selected tab provider; previews and tests drive it directly.
class GlassNav extends StatelessWidget {
  const GlassNav({super.key, required this.selected, required this.onSelect, this.solid = false});
  final bool solid;
  final int selected;
  final void Function(int) onSelect;

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final Kit kit = Kit.of(context);
    final List<(IconData, IconData, String)> items =
        <(IconData, IconData, String)>[
          (Icons.view_list_outlined, Icons.view_list, l10n.tabSessions),
          (Icons.chat_bubble_outline, Icons.chat_bubble, l10n.tabChat),
          (Icons.insights_outlined, Icons.insights, l10n.tabVisualize),
        ];
    return GlassBar(
      top: false,
      solid: solid,
      child: SafeArea(
        top: false,
        child: SizedBox(
          height: 64,
          child: LayoutBuilder(
            builder: (BuildContext context, BoxConstraints box) {
              final double width = box.maxWidth / items.length;
              const double pill = 64;
              return Stack(
                children: <Widget>[
                  // One pill that slides between the tabs with a spring-like curve.
                  AnimatedPositioned(
                    duration: motionDuration(
                      context,
                      const Duration(milliseconds: 380),
                    ),
                    curve: Curves.easeOutBack,
                    left: selected * width + (width - pill) / 2,
                    top: 8,
                    width: pill,
                    height: 32,
                    child: DecoratedBox(
                      decoration: BoxDecoration(
                        color: kit.selection,
                        borderRadius: BorderRadius.circular(Radii.full),
                      ),
                    ),
                  ),
                  Row(
                    children: <Widget>[
                      for (int i = 0; i < items.length; i++)
                        Expanded(
                          child: Semantics(
                            button: true,
                            selected: i == selected,
                            label: items[i].$3,
                            excludeSemantics: true,
                            child: GestureDetector(
                              behavior: HitTestBehavior.opaque,
                              onTap: () {
                                if (i != selected) {
                                  unawaited(HapticFeedback.selectionClick());
                                }
                                onSelect(i);
                              },
                              child: Column(
                                mainAxisAlignment: MainAxisAlignment.center,
                                children: <Widget>[
                                  SizedBox(
                                    height: 32,
                                    child: Center(
                                      child: AnimatedSwitcher(
                                        duration: motionDuration(
                                          context,
                                          Motion.ui,
                                        ),
                                        switchInCurve: Curves.easeOutBack,
                                        transitionBuilder:
                                            (
                                              Widget child,
                                              Animation<double> a,
                                            ) => ScaleTransition(
                                              scale: Tween<double>(
                                                begin: 0.7,
                                                end: 1,
                                              ).animate(a),
                                              child: FadeTransition(
                                                opacity: a,
                                                child: child,
                                              ),
                                            ),
                                        child: Icon(
                                          i == selected
                                              ? items[i].$2
                                              : items[i].$1,
                                          key: ValueKey<bool>(i == selected),
                                          size: 22,
                                          color: i == selected
                                              ? kit.accentText
                                              : kit.textMuted,
                                        ),
                                      ),
                                    ),
                                  ),
                                  AnimatedDefaultTextStyle(
                                    duration: motionDuration(
                                      context,
                                      Motion.ui,
                                    ),
                                    style: DefaultTextStyle.of(context).style.merge(kit.label).copyWith(
                                      fontSize: 12,
                                      fontWeight: i == selected
                                          ? FontWeight.w700
                                          : FontWeight.w500,
                                      color: i == selected
                                          ? kit.accentText
                                          : kit.textMuted,
                                    ),
                                    child: Text(
                                      items[i].$3,
                                      maxLines: 1,
                                      overflow: TextOverflow.ellipsis,
                                    ),
                                  ),
                                ],
                              ),
                            ),
                          ),
                        ),
                    ],
                  ),
                ],
              );
            },
          ),
        ),
      ),
    );
  }
}
