import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/net/bridge_client.dart';
import '../connection/connection.dart';

/// A command the session can run: Claude Code's built-ins, plus the user's, the project's and plugins' commands.
class SlashCommand {
  const SlashCommand({required this.name, required this.description, this.argumentHint});
  final String name;
  final String description;
  final String? argumentHint;
}

/// Claude Code's built-ins, the same list the Mac keeps. The phone offers these until the Mac answers `commands.list`
/// with the session's real list (the user's and the project's own commands too), and keeps them on an older Mac.
const List<SlashCommand> builtInSlashCommands = <SlashCommand>[
  SlashCommand(name: 'clear', description: 'Clear the conversation and start fresh'),
  SlashCommand(name: 'compact', description: 'Summarise the conversation to free up context'),
  SlashCommand(name: 'context', description: 'Show how much context is used'),
  SlashCommand(name: 'cost', description: 'Show the cost of this session'),
  SlashCommand(name: 'usage', description: 'Show plan usage limits'),
  SlashCommand(name: 'model', description: 'Change the model'),
  SlashCommand(name: 'effort', description: 'Change the effort level'),
  SlashCommand(name: 'review', description: 'Review a pull request'),
  SlashCommand(name: 'security-review', description: 'Security review of pending changes'),
  SlashCommand(name: 'init', description: 'Write a CLAUDE.md for this project'),
  SlashCommand(name: 'memory', description: 'Edit memory files'),
  SlashCommand(name: 'rewind', description: 'Go back to an earlier point'),
  SlashCommand(name: 'rename', description: 'Rename this session'),
  SlashCommand(name: 'status', description: 'Show version, model and account'),
  SlashCommand(name: 'todos', description: 'Show the current task list'),
  SlashCommand(name: 'help', description: 'Show help'),
];

/// The commands of one session: the built-ins at once, the Mac's list when it arrives (and again after a reconnect).
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
      // An older Mac does not know the op: the built-ins stay.
      debugPrint('SlashCommandsNotifier($sessionId): ${error.code} ${error.message}');
    }
  }
}

/// Commands matching what is typed after the slash: prefix matches first, then the rest.
List<SlashCommand> matchSlashCommands(String typed, List<SlashCommand> commands, {int limit = 6}) {
  final String q = typed.toLowerCase();
  if (q.isEmpty) return commands.take(limit).toList();
  final List<SlashCommand> prefix = commands.where((SlashCommand c) => c.name.toLowerCase().startsWith(q)).toList();
  final List<SlashCommand> rest = commands
      .where((SlashCommand c) => !c.name.toLowerCase().startsWith(q) && (c.name.toLowerCase().contains(q) || c.description.toLowerCase().contains(q)))
      .toList();
  return <SlashCommand>[...prefix, ...rest].take(limit).toList();
}
