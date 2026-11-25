import 'dart:async';

import 'package:flutter/material.dart';

import '../../app/theme/tokens.dart';
import '../../core/protocol/models.dart';
import '../../l10n/app_localizations.dart';

const Duration pendingPickTimeout = Duration(seconds: 30);

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
  return version.isEmpty ? family : <String>[family, version.join('.')].join(' ');
}

String _command(String name, String argument) => <String>[name, argument].join(' ');

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
    this.models,
  });
  final String? model;
  final String? effort;
  final List<ModelChoice>? models;
  final bool enabled;
  final void Function(String text) onSend;

  @override
  State<ComposerControls> createState() => _ComposerControlsState();
}

class _ComposerControlsState extends State<ComposerControls> {
  String? _pickedEffort;
  String? _pickedModel;
  Timer? _effortTimer;
  Timer? _modelTimer;

  @override
  void didUpdateWidget(ComposerControls oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.effort != widget.effort) _dropEffortPick();
    if (oldWidget.model != widget.model) _dropModelPick();
  }

  @override
  void dispose() {
    _effortTimer?.cancel();
    _modelTimer?.cancel();
    super.dispose();
  }

  void _dropEffortPick() {
    _effortTimer?.cancel();
    _pickedEffort = null;
  }

  void _dropModelPick() {
    _modelTimer?.cancel();
    _pickedModel = null;
  }

  void _holdEffortPick(String level) {
    _dropEffortPick();
    if (level == widget.effort) return;
    _pickedEffort = level;
    _effortTimer = Timer(pendingPickTimeout, () {
      if (mounted) setState(_dropEffortPick);
    });
  }

  void _holdModelPick(String id) {
    _dropModelPick();
    if (id == widget.model) return;
    _pickedModel = id;
    _modelTimer = Timer(pendingPickTimeout, () {
      if (mounted) setState(_dropModelPick);
    });
  }

  String _modelName(String id) {
    for (final ModelChoice choice in widget.models ?? const <ModelChoice>[]) {
      if (choice.id == id) return choice.name;
    }
    return modelShortName(id);
  }

  List<_Group> _modelGroups(AppL10n l10n) {
    final List<ModelChoice>? catalog = widget.models;
    if (catalog != null && catalog.isNotEmpty) {
      return <_Group>[
        _Group(null, <_Choice>[
          for (final ModelChoice m in catalog) _Choice(m.id, m.name),
        ]),
      ];
    }
    return <_Group>[
      _Group(null, <_Choice>[
        for (final String m in currentModels) _Choice(m, modelShortName(m)),
      ]),
      _Group(l10n.chatModelOlder, <_Choice>[
        for (final String m in olderModels)
          _Choice(m, modelShortName(m), detail: m),
      ]),
    ];
  }

  Future<void> _pickModel() async {
    final AppL10n l10n = AppL10n.of(context);
    final String? id = await _sheet(
      context,
      title: l10n.chatModelSheetTitle,
      groups: _modelGroups(l10n),
      current: _pickedModel ?? widget.model,
    );
    if (id == null || !mounted) return;
    setState(() => _holdModelPick(id));
    widget.onSend(_command('/model', id));
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
    setState(() => _holdEffortPick(level));
    widget.onSend(_command('/effort', level));
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
            label: model == null ? l10n.chatModelChip : _modelName(model),
            pending: _pickedModel != null,
            onTap: widget.enabled ? () => unawaited(_pickModel()) : null,
          ),
          const SizedBox(width: Space.xs),
          _Chip(
            key: const ValueKey<String>('effort-chip'),
            icon: Icons.speed,
            label: effort == null
                ? l10n.chatEffortChip
                : effortTitle(l10n, effort),
            pending: _pickedEffort != null,
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
    this.pending = false,
  });
  final IconData icon;
  final String label;
  final bool pending;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    final Kit kit = Kit.of(context);
    final AppL10n l10n = AppL10n.of(context);
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
              if (pending) ...<Widget>[
                const SizedBox(width: Space.xs),
                Icon(Icons.schedule, size: 14, color: kit.textMuted),
                const SizedBox(width: 2),
                Text(
                  l10n.chatPickPending,
                  style: kit.meta.copyWith(color: kit.textMuted),
                ),
              ] else
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
