import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:path_provider/path_provider.dart';

import '../../core/protocol/models.dart';
import '../macs/session_ref.dart';

abstract interface class ArchiveStore {
  Future<Map<String, int>> load();
  Future<void> save(Map<String, int> archived);
}

class DiskArchiveStore implements ArchiveStore {
  DiskArchiveStore([Future<Directory> Function()? root]) : _root = root ?? getApplicationSupportDirectory;
  final Future<Directory> Function() _root;

  Future<File> _file() async => File('${(await _root()).path}/archived_sessions.json');

  @override
  Future<Map<String, int>> load() async {
    try {
      final File file = await _file();
      if (!file.existsSync()) return <String, int>{};
      final Object? decoded = jsonDecode(await file.readAsString());
      if (decoded is! Map<String, Object?>) return <String, int>{};
      return <String, int>{for (final MapEntry<String, Object?> e in decoded.entries) if (e.value is num) e.key: (e.value! as num).toInt()};
    } on Object catch (error) {
      debugPrint('ArchiveStore.load: $error');
      return <String, int>{};
    }
  }

  @override
  Future<void> save(Map<String, int> archived) async {
    try {
      final File file = await _file();
      await file.parent.create(recursive: true);
      final File scratch = File('${file.path}.tmp');
      await scratch.writeAsString(jsonEncode(archived), flush: true);
      await scratch.rename(file.path);
    } on Object catch (error) {
      debugPrint('ArchiveStore.save: $error');
    }
  }
}

class MemoryArchiveStore implements ArchiveStore {
  MemoryArchiveStore([Map<String, int>? initial]) : saved = initial ?? <String, int>{};
  Map<String, int> saved;

  @override
  Future<Map<String, int>> load() async => Map<String, int>.of(saved);

  @override
  Future<void> save(Map<String, int> archived) async => saved = Map<String, int>.of(archived);
}

final Provider<ArchiveStore> archiveStoreProvider = Provider<ArchiveStore>((Ref ref) => DiskArchiveStore());

final NotifierProvider<ArchivedSessionsNotifier, Map<String, int>> archivedSessionsProvider =
    NotifierProvider<ArchivedSessionsNotifier, Map<String, int>>(ArchivedSessionsNotifier.new);

class ArchivedSessionsNotifier extends Notifier<Map<String, int>> {
  Future<void>? _loading;

  @override
  Map<String, int> build() {
    final ArchiveStore store = ref.read(archiveStoreProvider);
    _loading = Future<void>(() async {
      final Map<String, int> saved = await store.load();
      if (ref.mounted) state = <String, int>{...saved, ...state};
    });
    return <String, int>{};
  }

  Future<void> removeScope(String scope) async {
    await _loading;
    if (!ref.mounted) return;
    final Map<String, int> kept = <String, int>{for (final MapEntry<String, int> e in state.entries) if (!keyInScope(scope, e.key)) e.key: e.value};
    if (kept.length == state.length) return;
    state = kept;
    await ref.read(archiveStoreProvider).save(kept);
  }

  void archive(String id) {
    state = <String, int>{...state, id: DateTime.now().millisecondsSinceEpoch};
    _persist();
  }

  void restore(String id) {
    state = <String, int>{...state}..remove(id);
    _persist();
  }

  void _persist() => ref.read(archiveStoreProvider).save(state);
}

Set<String> hiddenSessionIds(Map<String, int> archived, List<Session> sessions) => hiddenSessionIdsIn(archived, sessions, '');

Set<String> hiddenSessionIdsIn(Map<String, int> archived, List<Session> sessions, String scope) => <String>{
      for (final Session s in sessions)
        if (archived[scope.isEmpty ? s.id : '$scope/${s.id}'] case final int at when s.lastActivity.millisecondsSinceEpoch <= at) s.id,
    };
