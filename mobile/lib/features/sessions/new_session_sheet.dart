import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/tokens.dart';
import '../../app/widgets/common.dart';
import '../../core/net/bridge_client.dart';
import '../../l10n/app_localizations.dart';
import '../connection/connection.dart';

/// Recent folders on the Mac, for "Session mới".
final FutureProvider<List<String>> recentProjectsProvider = FutureProvider.autoDispose<List<String>>((Ref ref) async {
  final BridgeClient? client = ref.watch(bridgeClientProvider);
  if (client == null) throw const BridgeException('disconnected', 'Chưa kết nối tới Mac.');
  return client.projectsRecent();
});

const List<String> newSessionModels = <String>['default', 'opus', 'sonnet', 'haiku'];

Future<void> showNewSessionSheet(BuildContext context) {
  return showModalBottomSheet<void>(
    context: context,
    isScrollControlled: true,
    useSafeArea: true,
    showDragHandle: true,
    builder: (BuildContext context) => const _NewSessionSheet(),
  );
}

class _NewSessionSheet extends ConsumerStatefulWidget {
  const _NewSessionSheet();

  @override
  ConsumerState<_NewSessionSheet> createState() => _NewSessionSheetState();
}

class _NewSessionSheetState extends ConsumerState<_NewSessionSheet> {
  String? _folder;
  String _model = 'default';
  bool _starting = false;

  Future<void> _start() async {
    final String? folder = _folder;
    if (folder == null) return;
    final AppL10n l10n = AppL10n.of(context);
    final ScaffoldMessengerState messenger = ScaffoldMessenger.of(context);
    final NavigatorState navigator = Navigator.of(context);
    final bool? confirmed = await showDialog<bool>(
      context: context,
      builder: (BuildContext context) => AlertDialog(
        title: Text(l10n.newSessionConfirmTitle),
        content: Text(l10n.newSessionConfirmBody(folder.split('/').last)),
        actions: <Widget>[
          TextButton(onPressed: () => Navigator.pop(context, false), child: Text(l10n.cancel)),
          TextButton(onPressed: () => Navigator.pop(context, true), child: Text(l10n.newSessionOpen)),
        ],
      ),
    );
    if (confirmed != true) return;
    setState(() => _starting = true);
    try {
      await ref.read(bridgeClientProvider)!.sessionNew(folder, model: _model == 'default' ? null : _model);
      navigator.pop();
      messenger.showSnackBar(SnackBar(content: Text(l10n.newSessionStarted)));
    } on BridgeException {
      if (mounted) setState(() => _starting = false);
      messenger.showSnackBar(SnackBar(content: Text(l10n.newSessionFailed)));
    }
  }

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final AsyncValue<List<String>> projects = ref.watch(recentProjectsProvider);
    return Padding(
      padding: EdgeInsets.only(bottom: MediaQuery.viewInsetsOf(context).bottom),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Padding(padding: const EdgeInsets.symmetric(horizontal: Space.lg), child: Text(l10n.newSession, style: TypeToken.titleMd)),
          const SizedBox(height: Space.md),
          Padding(padding: const EdgeInsets.symmetric(horizontal: Space.lg), child: PanelTitle(l10n.newSessionFolder)),
          Flexible(
            child: projects.when(
              loading: () => const SkeletonRows(count: 4),
              error: (Object error, StackTrace stack) => ErrorState(message: l10n.loadFailed, onRetry: () => ref.invalidate(recentProjectsProvider)),
              data: (List<String> folders) => folders.isEmpty
                  ? Padding(padding: const EdgeInsets.all(Space.lg), child: Text(l10n.newSessionNoProjects, style: TypeToken.bodyMd))
                  : ListView(
                      shrinkWrap: true,
                      children: <Widget>[
                        for (final String folder in folders)
                          Semantics(
                            button: true,
                            selected: folder == _folder,
                            label: folder,
                            excludeSemantics: true,
                            onTap: () => setState(() => _folder = folder),
                            child: InkWell(
                              onTap: () => setState(() => _folder = folder),
                              child: ConstrainedBox(
                                constraints: const BoxConstraints(minHeight: Metric.minTarget),
                                child: Padding(
                                  padding: const EdgeInsets.symmetric(horizontal: Space.lg, vertical: Space.sm),
                                  child: Row(children: <Widget>[
                                    Icon(folder == _folder ? Icons.radio_button_checked : Icons.radio_button_unchecked,
                                        size: 20, color: folder == _folder ? Token.primary : Token.muted),
                                    const SizedBox(width: Space.md),
                                    Expanded(child: Text(folder, style: TypeToken.codeSm.copyWith(color: Token.onSurface))),
                                  ]),
                                ),
                              ),
                            ),
                          ),
                      ],
                    ),
            ),
          ),
          Padding(padding: const EdgeInsets.fromLTRB(Space.lg, Space.md, Space.lg, 0), child: PanelTitle(l10n.newSessionModel)),
          Padding(
            padding: const EdgeInsets.all(Space.lg),
            child: Wrap(spacing: Space.sm, runSpacing: Space.sm, children: <Widget>[
              for (final String model in newSessionModels)
                ChoiceChip(
                  label: Text(model == 'default' ? l10n.modelDefault : model),
                  selected: _model == model,
                  onSelected: (_) => setState(() => _model = model),
                  showCheckmark: false,
                  labelStyle: TypeToken.labelSm.copyWith(color: _model == model ? Token.primary : Token.muted, fontSize: 13),
                  backgroundColor: Token.surface,
                  selectedColor: Token.surfaceRaised,
                  side: const BorderSide(color: Token.border),
                  materialTapTargetSize: MaterialTapTargetSize.padded,
                ),
            ]),
          ),
          Padding(
            padding: const EdgeInsets.fromLTRB(Space.lg, 0, Space.lg, Space.lg),
            child: SizedBox(
              width: double.infinity,
              child: PrimaryButton(label: l10n.newSessionOpen, onPressed: _folder == null || _starting ? null : () => unawaited(_start())),
            ),
          ),
        ],
      ),
    );
  }
}
