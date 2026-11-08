import 'package:flutter_secure_storage/flutter_secure_storage.dart';

import 'mac_profile.dart';

/// Where secrets live. The real one is the platform keychain; tests use memory.
abstract class SecretStore {
  Future<String?> read(String key);
  Future<void> write(String key, String value);
  Future<void> delete(String key);
}

class KeychainSecretStore implements SecretStore {
  const KeychainSecretStore([this._storage = const FlutterSecureStorage()]);
  final FlutterSecureStorage _storage;

  @override
  Future<String?> read(String key) => _storage.read(key: key);

  @override
  Future<void> write(String key, String value) => _storage.write(key: key, value: value);

  @override
  Future<void> delete(String key) => _storage.delete(key: key);
}

class MemorySecretStore implements SecretStore {
  final Map<String, String> values = <String, String>{};

  @override
  Future<String?> read(String key) async => values[key];

  @override
  Future<void> write(String key, String value) async => values[key] = value;

  @override
  Future<void> delete(String key) async => values.remove(key);
}

/// The list of paired Macs (tokens included) and which one is active.
class MacStore {
  const MacStore(this._secrets);
  final SecretStore _secrets;

  static const String _macsKey = 'macs.v1';
  static const String _activeKey = 'macs.active';

  Future<List<MacProfile>> load() async {
    final String? raw = await _secrets.read(_macsKey);
    if (raw == null) return <MacProfile>[];
    return decodeProfiles(raw);
  }

  Future<String?> activeId() => _secrets.read(_activeKey);

  Future<void> save(List<MacProfile> profiles, {String? activeId}) async {
    await _secrets.write(_macsKey, encodeProfiles(profiles));
    if (activeId != null) await _secrets.write(_activeKey, activeId);
  }

  Future<void> setActive(String id) => _secrets.write(_activeKey, id);

  /// No computer is in use: forget which one was.
  Future<void> clearActive() => _secrets.delete(_activeKey);
}
