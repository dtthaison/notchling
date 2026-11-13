import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/alerts.dart';
import '../../app/providers.dart';
import '../../core/net/ws_transport.dart';
import '../../core/pairing/mac_profile.dart';
import '../../core/pairing/mac_store.dart';
import '../chat/chat_cache.dart';
import '../sessions/archive_store.dart';

class MacsState {
  const MacsState({this.macs = const <MacProfile>[], this.activeId, this.loaded = false, this.scopes = const <String, String>{}});
  final List<MacProfile> macs;
  final String? activeId;
  final bool loaded;

  final Map<String, String> scopes;

  MacProfile? byId(String id) {
    for (final MacProfile mac in macs) {
      if (mac.id == id) return mac;
    }
    return null;
  }

  String scopeOf(String id) => scopes[id] ?? id;

  String get activeScope => activeId == null ? '' : scopeOf(activeId!);

  String? keyOf(String id) {
    final MacProfile? mac = byId(id);
    return mac == null ? null : '${mac.id}|${mac.token}|${mac.hosts.join(',')}|${mac.port}|${mac.fingerprint}';
  }

  MacsState copyWith({List<MacProfile>? macs, String? activeId, Map<String, String>? scopes}) =>
      MacsState(macs: macs ?? this.macs, activeId: activeId ?? this.activeId, loaded: true, scopes: scopes ?? this.scopes);

  MacProfile? get active {
    for (final MacProfile mac in macs) {
      if (mac.id == activeId) return mac;
    }
    return null;
  }

  String? get activeKey => activeId == null ? null : keyOf(activeId!);
}

final NotifierProvider<MacsNotifier, MacsState> macsProvider = NotifierProvider<MacsNotifier, MacsState>(MacsNotifier.new);

class MacsNotifier extends Notifier<MacsState> {
  late final MacStore _store = MacStore(ref.read(secretStoreProvider));
  static const String _scopesKey = 'macs.scopes';

  @override
  MacsState build() {
    _load();
    return const MacsState();
  }

  Future<void> _load() async {
    final SecretStore secrets = ref.read(secretStoreProvider);
    final List<MacProfile> macs = await _store.load();
    final String? active = await _store.activeId();
    final String? valid = macs.any((MacProfile m) => m.id == active) ? active : (macs.isEmpty ? null : macs.first.id);
    final Map<String, String>? saved = await _loadScopes(secrets);
    final Map<String, String> scopes = saved ?? <String, String>{for (final MacProfile m in macs) m.id: m.id == valid ? '' : m.id};
    if (!ref.mounted) return;
    state = MacsState(macs: macs, activeId: valid, loaded: true, scopes: scopes);
    if (saved == null) await secrets.write(_scopesKey, jsonEncode(scopes));
  }

  static Future<Map<String, String>?> _loadScopes(SecretStore secrets) async {
    final String? raw = await secrets.read(_scopesKey);
    if (raw == null) return null;
    try {
      final Object? decoded = jsonDecode(raw);
      if (decoded is Map<String, Object?>) return <String, String>{for (final MapEntry<String, Object?> e in decoded.entries) if (e.value is String) e.key: e.value! as String};
    } on FormatException catch (error) {
      debugPrint('MacsNotifier: scopes unreadable: $error');
    }
    return <String, String>{};
  }

  Future<void> _saveScopes(Map<String, String> scopes) => ref.read(secretStoreProvider).write(_scopesKey, jsonEncode(scopes));

  Future<void> add(MacProfile mac) async {
    final List<MacProfile> macs = <MacProfile>[...state.macs.where((MacProfile m) => m.id != mac.id), mac];
    final Map<String, String> scopes = <String, String>{...state.scopes, mac.id: state.scopes[mac.id] ?? mac.id};
    state = MacsState(macs: macs, activeId: mac.id, loaded: true, scopes: scopes);
    await _store.save(macs, activeId: mac.id);
    await _saveScopes(scopes);
  }

  Future<void> setActive(String id) async {
    state = state.copyWith(activeId: id);
    await _store.setActive(id);
  }

  Future<void> remove(String id) async {
    final String scope = state.scopeOf(id);
    final List<MacProfile> macs = state.macs.where((MacProfile m) => m.id != id).toList();
    final String? active = state.activeId == id ? (macs.isEmpty ? null : macs.first.id) : state.activeId;
    final Map<String, String> scopes = <String, String>{...state.scopes}..remove(id);
    state = MacsState(macs: macs, activeId: active, loaded: true, scopes: scopes);
    await ref.read(chatCacheProvider).deleteScope(scope);
    await ref.read(archivedSessionsProvider.notifier).removeScope(scope);
    await ref.read(alertPrefsProvider.notifier).removeScope(scope);
    await _store.save(macs, activeId: active);
    if (active == null) await _store.clearActive();
    await _saveScopes(scopes);
  }

  Future<void> rememberRoute(String id, Endpoint route) async {
    final List<MacProfile> macs = state.macs.map((MacProfile m) {
      if (m.id != id) return m;
      if (route.kind == RouteKind.tunnel) return m.lastGoodWasTunnel ? m : m.withLastGoodTunnel();
      return m.lastGoodHost == route.host && !m.lastGoodWasTunnel ? m : m.withLastGoodHost(route.host);
    }).toList();
    state = state.copyWith(macs: macs);
    await _store.save(macs);
  }
}
