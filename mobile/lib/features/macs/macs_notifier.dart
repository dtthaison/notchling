import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/providers.dart';
import '../../core/net/ws_transport.dart';
import '../../core/pairing/mac_profile.dart';
import '../../core/pairing/mac_store.dart';

class MacsState {
  const MacsState({this.macs = const <MacProfile>[], this.activeId, this.loaded = false});
  final List<MacProfile> macs;
  final String? activeId;
  final bool loaded;

  MacProfile? get active {
    for (final MacProfile mac in macs) {
      if (mac.id == activeId) return mac;
    }
    return null;
  }

  /// Changes when the active Mac, or anything needed to connect to it, changes (not when only the
  /// remembered host does, which would restart the connection for nothing).
  String? get activeKey {
    final MacProfile? mac = active;
    return mac == null ? null : '${mac.id}|${mac.token}|${mac.hosts.join(',')}|${mac.port}|${mac.fingerprint}';
  }
}

final NotifierProvider<MacsNotifier, MacsState> macsProvider = NotifierProvider<MacsNotifier, MacsState>(MacsNotifier.new);

/// The paired Macs, loaded from the keychain once.
class MacsNotifier extends Notifier<MacsState> {
  late final MacStore _store = MacStore(ref.read(secretStoreProvider));

  @override
  MacsState build() {
    _load();
    return const MacsState();
  }

  Future<void> _load() async {
    final List<MacProfile> macs = await _store.load();
    final String? active = await _store.activeId();
    final String? valid = macs.any((MacProfile m) => m.id == active) ? active : (macs.isEmpty ? null : macs.first.id);
    state = MacsState(macs: macs, activeId: valid, loaded: true);
  }

  /// Adds (or replaces, by id) a Mac and makes it the active one.
  Future<void> add(MacProfile mac) async {
    final List<MacProfile> macs = <MacProfile>[...state.macs.where((MacProfile m) => m.id != mac.id), mac];
    state = MacsState(macs: macs, activeId: mac.id, loaded: true);
    await _store.save(macs, activeId: mac.id);
  }

  Future<void> setActive(String id) async {
    state = MacsState(macs: state.macs, activeId: id, loaded: true);
    await _store.setActive(id);
  }

  Future<void> remove(String id) async {
    final List<MacProfile> macs = state.macs.where((MacProfile m) => m.id != id).toList();
    final String? active = state.activeId == id ? (macs.isEmpty ? null : macs.first.id) : state.activeId;
    state = MacsState(macs: macs, activeId: active, loaded: true);
    await _store.save(macs, activeId: active);
  }

  /// Remembers which route answered (a LAN host or the tunnel), so it is tried first next time.
  Future<void> rememberRoute(String id, Endpoint route) async {
    final List<MacProfile> macs = state.macs.map((MacProfile m) {
      if (m.id != id) return m;
      if (route.kind == RouteKind.tunnel) return m.lastGoodWasTunnel ? m : m.withLastGoodTunnel();
      return m.lastGoodHost == route.host && !m.lastGoodWasTunnel ? m : m.withLastGoodHost(route.host);
    }).toList();
    state = MacsState(macs: macs, activeId: state.activeId, loaded: true);
    await _store.save(macs);
  }
}
