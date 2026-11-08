// Two paired computers for tests of what the phone keeps per computer (decision 0012).
import 'package:agent_console/app/providers.dart';
import 'package:agent_console/core/net/ws_transport.dart';
import 'package:agent_console/core/pairing/mac_profile.dart';
import 'package:agent_console/core/pairing/mac_store.dart';
import 'package:agent_console/features/chat/chat_cache.dart';
import 'package:agent_console/features/sessions/archive_store.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:flutter_test/flutter_test.dart';

import 'fake_ws.dart';

const MacProfile macA = MacProfile(
  id: 'aaaaaaaaaaaaaaaa',
  name: 'Mac A',
  hosts: <String>['10.0.0.1'],
  port: 47821,
  fingerprint: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  token: 'tok-a',
);

const MacProfile macB = MacProfile(
  id: 'bbbbbbbbbbbbbbbb',
  name: 'Linux B',
  hosts: <String>['10.0.0.2'],
  port: 47821,
  fingerprint: 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
  token: 'tok-b',
);

/// A computer whose `s1` is view-only and can be taken over. It answers the usual ops at once, and a takeover only
/// when the test says so (`reply` on the socket).
FakeWs computerSocket() => FakeWs(onSend: (FakeWs socket, Map<String, Object?> message) {
      final Object id = message['id']!;
      switch (message['op']) {
        case 'hello':
          socket.reply(id, data: <String, Object?>{'version': 1});
        case 'sessions.list':
          socket.reply(id, data: <String, Object?>{
            'sessions': <Object?>[sessionJson('s1', state: 'done', control: 'viewOnly', takeover: <String, Object?>{'available': true})],
          });
        case 'chat.get':
          socket.reply(id, data: <String, Object?>{'messages': <Object?>[], 'hasMore': false});
        case 'subscribe':
          socket.reply(id, data: <String, Object?>{'subscribed': true});
        case 'commands.list':
          socket.reply(id, data: <String, Object?>{'commands': <Object?>[]});
      }
    });

/// Both computers paired with [active] in use, each reached through its own fake socket. [secrets] starts the
/// phone from stored state; [extra] replaces a store or a provider.
ProviderContainer pairedComputers(
  FakeWs a,
  FakeWs b, {
  MacProfile active = macA,
  MemorySecretStore? secrets,
  List<Override> extra = const <Override>[],
}) {
  final MemorySecretStore store = secrets ??
      (MemorySecretStore()
        ..values['macs.v1'] = encodeProfiles(<MacProfile>[macA, macB])
        ..values['macs.active'] = active.id);
  final ProviderContainer container = ProviderContainer(overrides: <Override>[
    secretStoreProvider.overrideWithValue(store),
    chatCacheProvider.overrideWithValue(MemoryChatCache()),
    archiveStoreProvider.overrideWithValue(MemoryArchiveStore()),
    wsConnectorProvider.overrideWithValue((Endpoint endpoint) async => endpoint.host == '10.0.0.2' ? b : a),
    networkChangesProvider.overrideWithValue(const Stream<void>.empty()),
    ...extra,
  ]);
  addTearDown(container.dispose);
  return container;
}
