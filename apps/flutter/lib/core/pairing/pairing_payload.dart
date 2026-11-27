import 'dart:convert';

import '../net/tunnel.dart';
import '../protocol/json.dart';

class PairingFormatException implements Exception {
  const PairingFormatException(this.message);
  final String message;

  @override
  String toString() => 'PairingFormatException: $message';
}

final RegExp _fingerprintPattern = RegExp(r'^[0-9a-f]{64}$');
final RegExp _codePattern = RegExp(r'^[A-Z0-9]{8}$');
final RegExp _hostPattern = RegExp(r'^[A-Za-z0-9.:\-_]+$');

class PairingPayload {
  const PairingPayload({required this.name, required this.port, required this.fingerprint, required this.hosts, required this.code, this.tunnel});

  factory PairingPayload.parse(String text) {
    final Object? decoded;
    try {
      decoded = jsonDecode(text.trim());
    } on FormatException {
      throw const PairingFormatException('not JSON');
    }
    if (decoded is! Map<String, Object?>) throw const PairingFormatException('not an object');
    return PairingPayload.fromJson(decoded);
  }

  factory PairingPayload.fromJson(Json json) {
    try {
      final int version = json.integer('v');
      if (version != 1 && version != 2) throw const PairingFormatException('unsupported version');
      final int port = json.integer('port');
      final String fingerprint = json.str('fp').toLowerCase();
      final List<String> hosts = json.strings('hosts').map((String h) => h.trim()).where((String h) => h.isNotEmpty).toList();
      final String code = json.str('code').trim().toUpperCase();
      return PairingPayload(
        name: (json.strOrNull('name') ?? 'Mac').trim(),
        port: port,
        fingerprint: fingerprint,
        hosts: hosts,
        code: code,
        tunnel: json['tunnel'] is Map<Object?, Object?> ? TunnelInfo.fromJson(json.object('tunnel')) : null,
      )..validate();
    } on FormatException catch (error) {
      throw PairingFormatException(error.message);
    }
  }

  final String name;
  final int port;
  final String fingerprint;
  final List<String> hosts;
  final String code;
  final TunnelInfo? tunnel;

  void validate() {
    if (port < 1 || port > 65535) throw const PairingFormatException('port out of range');
    if (!_fingerprintPattern.hasMatch(fingerprint)) throw const PairingFormatException('fingerprint must be 64 hex characters');
    if (hosts.isEmpty) throw const PairingFormatException('no hosts');
    if (hosts.any((String h) => !_hostPattern.hasMatch(h))) throw const PairingFormatException('bad host');
    if (!_codePattern.hasMatch(code)) throw const PairingFormatException('code must be 8 letters or digits');
    try {
      tunnel?.validate();
    } on FormatException catch (error) {
      throw PairingFormatException(error.message);
    }
  }
}
