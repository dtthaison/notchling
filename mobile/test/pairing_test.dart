import 'dart:convert';
import 'dart:typed_data';

import 'package:agent_console/core/net/ws_transport.dart';
import 'package:agent_console/core/pairing/mac_profile.dart';
import 'package:agent_console/core/pairing/mac_store.dart';
import 'package:agent_console/core/pairing/pairing_payload.dart';
import 'package:crypto/crypto.dart';
import 'package:flutter_test/flutter_test.dart';

const String _fp = '20d3c003d553a68bd969f5663f4b747be987a2874fe13bb025fe07710b829445';

String _json({int v = 1, int port = 47821, String fp = _fp, List<String> hosts = const <String>['10.0.0.199', 'mac.local'], String code = 'XRSCD75H'}) =>
    jsonEncode(<String, Object?>{'v': v, 'name': 'Triết-Kun', 'port': port, 'fp': fp, 'hosts': hosts, 'code': code});

void main() {
  group('PairingPayload', () {
    test('parses the Mac QR JSON', () {
      final PairingPayload payload = PairingPayload.parse(_json());
      expect(payload.name, 'Triết-Kun');
      expect(payload.port, 47821);
      expect(payload.hosts, <String>['10.0.0.199', 'mac.local']);
      expect(payload.code, 'XRSCD75H');
      expect(payload.fingerprint, _fp);
    });

    test('normalises case and whitespace', () {
      final PairingPayload payload = PairingPayload.parse(' ${_json(fp: _fp.toUpperCase(), code: 'xrscd75h')} ');
      expect(payload.fingerprint, _fp);
      expect(payload.code, 'XRSCD75H');
    });

    test('rejects every malformed shape', () {
      final List<String> bad = <String>[
        'not json',
        '[]',
        _json(v: 3),
        _json(port: 0),
        _json(port: 70000),
        _json(fp: 'abc'),
        _json(fp: 'z' * 64),
        _json(hosts: <String>[]),
        _json(hosts: <String>['bad host']),
        _json(hosts: <String>['a/b']),
        _json(code: 'SHORT'),
        _json(code: 'XRSCD75H9'),
        _json(code: 'XRSCD-5H'),
        '{"v":1,"port":1}',
      ];
      for (final String text in bad) {
        expect(() => PairingPayload.parse(text), throwsA(isA<PairingFormatException>()), reason: text);
      }
    });

    test('accepts IPv6 and .local hosts', () {
      expect(PairingPayload.parse(_json(hosts: <String>['fd7a:115c:a1e0::5', 'triet-kun.local'])).hosts, hasLength(2));
    });
  });

  group('fingerprint pinning', () {
    final Uint8List der = Uint8List.fromList(List<int>.generate(64, (int i) => i));
    final String fp = sha256.convert(der).toString();

    test('accepts the pinned certificate in any case', () {
      expect(certificateMatchesFingerprint(der, fp), isTrue);
      expect(certificateMatchesFingerprint(der, fp.toUpperCase()), isTrue);
      expect(certificateMatchesFingerprint(der, ' $fp '), isTrue);
    });

    test('rejects any other certificate or fingerprint', () {
      expect(certificateMatchesFingerprint(der, _fp), isFalse);
      expect(certificateMatchesFingerprint(Uint8List.fromList(<int>[...der, 1]), fp), isFalse);
      expect(certificateMatchesFingerprint(der, ''), isFalse);
      expect(certificateMatchesFingerprint(der, fp.substring(0, 63)), isFalse);
    });
  });

  group('MacStore', () {
    test('round-trips profiles and the active Mac', () async {
      final MemorySecretStore secrets = MemorySecretStore();
      final MacStore store = MacStore(secrets);
      const MacProfile mac = MacProfile(
          id: 'a', name: 'Mac A', hosts: <String>['10.0.0.1', 'a.local'], port: 1, fingerprint: _fp, token: 'secret-token', lastGoodHost: 'a.local');
      await store.save(<MacProfile>[mac], activeId: 'a');
      final List<MacProfile> loaded = await MacStore(secrets).load();
      expect(loaded.single.token, 'secret-token');
      expect(loaded.single.orderedHosts, <String>['a.local', '10.0.0.1'], reason: 'the host that worked goes first');
      expect(await store.activeId(), 'a');
    });

    test('a vanished last-good host does not break ordering', () {
      const MacProfile mac = MacProfile(id: 'a', name: 'n', hosts: <String>['x'], port: 1, fingerprint: _fp, token: 't', lastGoodHost: 'gone');
      expect(mac.orderedHosts, <String>['x']);
    });
  });
}
