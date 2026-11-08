import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:path_provider/path_provider.dart';

import '../../core/protocol/models.dart';

/// Which sessions the user archived on this phone, and when. Stored on the phone, not on the Mac: archiving is a
/// preference about this list (the Mac's own sessions are untouched), it works offline, and each phone keeps its own.
abstract interface class ArchiveStore {
  /// Session id to the time it was archived, in milliseconds.
  Future<Map<String, int>> load();
  Future<void> save(Map<String, int> archived);
}

/// A JSON file in the app's support folder. A file that cannot be read or written is logged and treated as empty.
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
  @override
  Map<String, int> build() {
    final ArchiveStore store = ref.read(archiveStoreProvider);
    Future<void>(() async {
      final Map<String, int> saved = await store.load();
      if (ref.mounted) state = <String, int>{...saved, ...state};
    });
    return <String, int>{};
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

/// The archived sessions still hidden: one that has moved since it was archived (new activity) comes back by itself.
Set<String> hiddenSessionIds(Map<String, int> archived, List<Session> sessions) => hiddenSessionIdsIn(archived, sessions, '');

/// [hiddenSessionIds] for the sessions of the computer whose storage scope is [scope]: the archive keys carry the
/// scope (decision 0012), so archiving `s1` on one computer leaves `s1` on another in the list. Returns bare ids.
Set<String> hiddenSessionIdsIn(Map<String, int> archived, List<Session> sessions, String scope) => <String>{
      for (final Session s in sessions)
        if (archived[scope.isEmpty ? s.id : '$scope/${s.id}'] case final int at when s.lastActivity.millisecondsSinceEpoch <= at) s.id,
    };
