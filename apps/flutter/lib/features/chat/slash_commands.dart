import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/l10n_holder.dart';
import '../../core/net/bridge_client.dart';
import '../../l10n/app_localizations.dart';
import '../connection/connection.dart';

class SlashCommand {
  const SlashCommand({required this.name, required this.description, this.argumentHint});
  final String name;
  final String description;
  final String? argumentHint;
}

List<SlashCommand> builtInSlashCommandsFor(AppL10n l10n) => <SlashCommand>[
  SlashCommand(name: 'clear', description: l10n.chatSlashClear),
  SlashCommand(name: 'compact', description: l10n.chatSlashCompact),
  SlashCommand(name: 'context', description: l10n.chatSlashContext),
  SlashCommand(name: 'cost', description: l10n.chatSlashCost),
  SlashCommand(name: 'usage', description: l10n.chatSlashUsage),
  SlashCommand(name: 'model', description: l10n.chatSlashModel),
  SlashCommand(name: 'effort', description: l10n.chatSlashEffort),
  SlashCommand(name: 'review', description: l10n.chatSlashReview),
  SlashCommand(name: 'security-review', description: l10n.chatSlashSecurityReview),
  SlashCommand(name: 'init', description: l10n.chatSlashInit),
  SlashCommand(name: 'memory', description: l10n.chatSlashMemory),
  SlashCommand(name: 'rewind', description: l10n.chatSlashRewind),
  SlashCommand(name: 'rename', description: l10n.chatSlashRename),
  SlashCommand(name: 'status', description: l10n.chatSlashStatus),
  SlashCommand(name: 'todos', description: l10n.chatSlashTodos),
  SlashCommand(name: 'help', description: l10n.chatSlashHelp),
];

final List<SlashCommand> builtInSlashCommands = builtInSlashCommandsFor(L10nHolder.current);

final slashCommandsProvider = NotifierProvider.family<SlashCommandsNotifier, List<SlashCommand>, String>(SlashCommandsNotifier.new);

class SlashCommandsNotifier extends Notifier<List<SlashCommand>> {
  SlashCommandsNotifier(this.sessionId);
  final String sessionId;

  @override
  List<SlashCommand> build() {
    final BridgeClient? client = ref.watch(bridgeClientProvider);
    ref.watch(connectedCountProvider);
    if (client != null && client.isConnected) unawaited(_load(client));
    return builtInSlashCommands;
  }

  Future<void> _load(BridgeClient client) async {
    try {
      final rows = await client.commandsList(sessionId);
      if (!ref.mounted || rows.isEmpty) return;
      state = <SlashCommand>[for (final r in rows) SlashCommand(name: r.name, description: r.description, argumentHint: r.argumentHint)];
    } on BridgeException catch (error) {
      debugPrint('SlashCommandsNotifier($sessionId): ${error.code} ${error.message}');
    }
  }
}

List<SlashCommand> matchSlashCommands(String typed, List<SlashCommand> commands, {int limit = 6}) {
  final String q = typed.toLowerCase();
  if (q.isEmpty) return commands.take(limit).toList();
  final List<SlashCommand> prefix = commands.where((SlashCommand c) => c.name.toLowerCase().startsWith(q)).toList();
  final List<SlashCommand> rest = commands
      .where((SlashCommand c) => !c.name.toLowerCase().startsWith(q) && (c.name.toLowerCase().contains(q) || c.description.toLowerCase().contains(q)))
      .toList();
  return <SlashCommand>[...prefix, ...rest].take(limit).toList();
}
