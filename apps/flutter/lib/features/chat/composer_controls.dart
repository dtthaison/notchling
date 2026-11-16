import 'dart:async';

import 'package:flutter/material.dart';

import '../../app/theme/tokens.dart';
import '../../l10n/app_localizations.dart';

const List<String> currentModels = <String>[
  'claude-opus-5-5',
  'claude-fable-5-2',
  'claude-sonnet-5-5',
  'claude-haiku-5-5',
];

const List<String> olderModels = <String>[
  'claude-haiku-4-5',
  'claude-sonnet-5',
  'claude-opus-5',
  'claude-fable-5-1',
  'claude-fable-5',
  'claude-opus-4-8',
  'claude-opus-4-7',
  'claude-opus-4-6',
  'claude-sonnet-4-6',
];

const List<String> effortLevels = <String>['low', 'medium', 'high', 'xhigh', 'max'];

String modelShortName(String id) {
  final List<String> parts = id.replaceFirst('claude-', '').split('-');
  if (parts.isEmpty || parts.first.isEmpty) return id;
  final String family = parts.first[0].toUpperCase() + parts.first.substring(1);
  final List<String> version = <String>[];
  for (final String part in parts.skip(1)) {
    if (part.length > 2 || int.tryParse(part) == null) break;
    version.add(part);
  }
  return version.isEmpty ? family : '$family ${version.join('.')}';
}

String effortTitle(AppL10n l10n, String level) => switch (level) {
  'low' => l10n.chatEffortLow,
  'medium' => l10n.chatEffortMedium,
  'high' => l10n.chatEffortHigh,
  'xhigh' => l10n.chatEffortXhigh,
  'max' => l10n.chatEffortMax,
  _ => level,
};

class ComposerControls extends StatefulWidget {
  const ComposerControls({
    super.key,
    required this.model,
    required this.effort,
    required this.enabled,
    required this.onSend,
  });
  final String? model;
  final String? effort;
  final bool enabled;
  final void Function(String text) onSend;

  @override
  State<ComposerControls> createState() => _ComposerControlsState();
}

class _ComposerControlsState extends State<ComposerControls> {
  String? _pickedEffort;
  String? _pickedModel;

  @override
  void didUpdateWidget(ComposerControls oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.effort != widget.effort) _pickedEffort = null;
    if (oldWidget.model != widget.model) _pickedModel = null;
  }

  Future<void> _pickModel() async {
    final AppL10n l10n = AppL10n.of(context);
    final String? id = await _sheet(
      context,
      title: l10n.chatModelSheetTitle,
      groups: <_Group>[
        _Group(null, <_Choice>[
          for (final String m in currentModels) _Choice(m, modelShortName(m)),
        ]),
        _Group(l10n.chatModelOlder, <_Choice>[
          for (final String m in olderModels)
            _Choice(m, modelShortName(m), detail: m),
        ]),
      ],
      current: _pickedModel ?? widget.model,
    );
    if (id == null || !mounted) return;
    setState(() => _pickedModel = id);
    widget.onSend('/model $id');
  }

  Future<void> _pickEffort() async {
    final AppL10n l10n = AppL10n.of(context);
    final String? level = await _sheet(
      context,
      title: l10n.chatEffortSheetTitle,
      groups: <_Group>[
        _Group(null, <_Choice>[
          for (final String e in effortLevels) _Choice(e, effortTitle(l10n, e)),
        ]),
      ],
      current: _pickedEffort ?? widget.effort,
    );
    if (level == null || !mounted) return;
    setState(() => _pickedEffort = level);
    widget.onSend('/effort $level');
  }

  Future<void> _compact() async {
    final AppL10n l10n = AppL10n.of(context);
    final bool? yes = await showDialog<bool>(
      context: context,
      builder: (BuildContext dialog) => AlertDialog(
        title: Text(l10n.chatCompactTitle),
        content: Text(l10n.chatCompactConfirm),
        actions: <Widget>[
          TextButton(
            onPressed: () => Navigator.of(dialog).pop(false),
            child: Text(l10n.appCancel),
          ),
          TextButton(
            onPressed: () => Navigator.of(dialog).pop(true),
            child: Text(l10n.chatCompactTooltip),
          ),
        ],
      ),
    );
    if (yes == true) widget.onSend('/compact');
  }

  @override
  Widget build(BuildContext context) {
    final Kit kit = Kit.of(context);
    final AppL10n l10n = AppL10n.of(context);
    final String? model = _pickedModel ?? widget.model;
    final String? effort = _pickedEffort ?? widget.effort;
    return SingleChildScrollView(
      scrollDirection: Axis.horizontal,
      child: Row(
        children: <Widget>[
          _Chip(
            key: const ValueKey<String>('model-chip'),
            icon: Icons.memory,
            label: model == null ? l10n.chatModelChip : modelShortName(model),
            onTap: widget.enabled ? () => unawaited(_pickModel()) : null,
          ),
          const SizedBox(width: Space.xs),
          _Chip(
            key: const ValueKey<String>('effort-chip'),
            icon: Icons.speed,
            label: effort == null
                ? l10n.chatEffortChip
                : effortTitle(l10n, effort),
            onTap: widget.enabled ? () => unawaited(_pickEffort()) : null,
          ),
          IconButton(
            key: const ValueKey<String>('compact-button'),
            onPressed: widget.enabled ? () => unawaited(_compact()) : null,
            tooltip: l10n.chatCompactTooltip,
            constraints: const BoxConstraints(
              minWidth: Metric.minTarget,
              minHeight: Metric.minTarget,
            ),
            icon: Icon(Icons.compress, size: 20, color: kit.textMuted),
          ),
        ],
      ),
    );
  }
}

class _Chip extends StatelessWidget {
  const _Chip({
    super.key,
    required this.icon,
    required this.label,
    required this.onTap,
  });
  final IconData icon;
  final String label;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    final Kit kit = Kit.of(context);
    return InkWell(
      borderRadius: BorderRadius.circular(Radii.sm),
      onTap: onTap,
      child: ConstrainedBox(
        constraints: const BoxConstraints(minHeight: Metric.minTarget),
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: Space.sm),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: <Widget>[
              Icon(icon, size: 14, color: kit.textMuted),
              const SizedBox(width: Space.xs),
              Text(label, style: kit.meta.copyWith(color: kit.textMuted)),
              Icon(Icons.expand_more, size: 14, color: kit.textMuted),
            ],
          ),
        ),
      ),
    );
  }
}

class _Choice {
  const _Choice(this.value, this.title, {this.detail});
  final String value;
  final String title;
  final String? detail;
}

class _Group {
  const _Group(this.title, this.choices);
  final String? title;
  final List<_Choice> choices;
}

Future<String?> _sheet(
  BuildContext context, {
  required String title,
  required List<_Group> groups,
  required String? current,
}) {
  return showModalBottomSheet<String>(
    context: context,
    isScrollControlled: true,
    useSafeArea: true,
    showDragHandle: true,
    builder: (BuildContext sheet) {
      final Kit kit = Kit.of(sheet);
      return SafeArea(
        child: ListView(
          shrinkWrap: true,
          padding: const EdgeInsets.symmetric(vertical: Space.sm),
          children: <Widget>[
            Padding(
              padding: const EdgeInsets.fromLTRB(
                Space.lg,
                Space.sm,
                Space.lg,
                Space.sm,
              ),
              child: Text(title, style: kit.heading),
            ),
            for (final _Group group in groups) ...<Widget>[
              if (group.title != null)
                Padding(
                  padding: const EdgeInsets.fromLTRB(
                    Space.lg,
                    Space.md,
                    Space.lg,
                    Space.xs,
                  ),
                  child: Text(
                    group.title!,
                    style: kit.section.copyWith(color: kit.textMuted),
                  ),
                ),
              for (final _Choice choice in group.choices)
                ListTile(
                  key: ValueKey<String>('choice-${choice.value}'),
                  title: Text(choice.title, style: kit.body),
                  subtitle: choice.detail == null
                      ? null
                      : Text(choice.detail!, style: kit.meta),
                  minVerticalPadding: Space.md,
                  trailing: choice.value == current
                      ? Icon(Icons.check, color: kit.accentText)
                      : null,
                  onTap: () => Navigator.of(sheet).pop(choice.value),
                ),
            ],
          ],
        ),
      );
    },
  );
}
