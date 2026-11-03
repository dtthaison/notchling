import 'package:agent_console/app/providers.dart';
import 'package:agent_console/app/theme/tokens.dart';
import 'package:agent_console/core/net/ws_transport.dart';
import 'package:agent_console/core/pairing/mac_profile.dart';
import 'package:agent_console/core/pairing/mac_store.dart';
import 'package:agent_console/features/chat/chat_cache.dart';
import 'package:agent_console/features/connection/connection.dart';
import 'package:agent_console/features/macs/macs_notifier.dart';
import 'package:agent_console/l10n/app_localizations.dart';
import 'package:flutter/material.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:flutter_test/flutter_test.dart';

import 'fake_ws.dart';

const MacProfile testMac = MacProfile(
  id: 'm',
  name: 'Mac thử',
  hosts: <String>['10.0.0.1'],
  port: 47821,
  fingerprint: '20d3c003d553a68bd969f5663f4b747be987a2874fe13bb025fe07710b829445',
  token: 'tok',
);

/// A container whose Mac is already paired and whose socket is [socket].
ProviderContainer containerWith(FakeWs socket, {WsConnector? connector, MacProfile mac = testMac, Stream<void>? networkChanges, ChatCache? cache}) {
  final MemorySecretStore secrets = MemorySecretStore()
    ..values['macs.v1'] = encodeProfiles(<MacProfile>[mac])
    ..values['macs.active'] = mac.id;
  final ProviderContainer container = ProviderContainer(overrides: <Override>[
    secretStoreProvider.overrideWithValue(secrets),
    chatCacheProvider.overrideWithValue(cache ?? MemoryChatCache()),
    wsConnectorProvider.overrideWithValue(connector ?? (Endpoint endpoint) async => socket),
    networkChangesProvider.overrideWithValue(networkChanges ?? const Stream<void>.empty()),
  ]);
  addTearDown(container.dispose);
  return container;
}

/// Loads the Macs and waits until the link is up.
Future<void> connect(ProviderContainer container) async {
  container.read(macsProvider);
  await until(() => container.read(macsProvider).loaded);
  container.listen<int>(connectedCountProvider, (int? a, int b) {});
  await until(() => container.read(bridgeClientProvider)?.isConnected ?? false);
}

Future<void> until(bool Function() condition, {Duration within = const Duration(seconds: 3)}) async {
  final DateTime deadline = DateTime.now().add(within);
  while (!condition()) {
    if (DateTime.now().isAfter(deadline)) fail('condition not met in $within');
    await Future<void>.delayed(const Duration(milliseconds: 5));
  }
}

/// A Mac that answers hello, lists the given sessions, and serves one chat page.
FakeWs macWithSessions(List<Map<String, Object?>> sessions, {List<Map<String, Object?>> chat = const <Map<String, Object?>>[]}) {
  return happyMac(auto: <String, Map<String, Object?>>{
    'sessions.list': <String, Object?>{'sessions': sessions},
    'chat.get': <String, Object?>{'messages': chat, 'hasMore': false},
    'session.message': <String, Object?>{'sent': true},
    'subscribe': <String, Object?>{'subscribed': true},
    'prompt.answer': <String, Object?>{'answered': true},
  });
}

/// Wraps a widget the way the app does: dark theme, localizations (Vietnamese unless [locale] says otherwise).
Widget host(Widget child, {Locale locale = const Locale('vi'), double textScale = 1}) {
  return MaterialApp(
    theme: buildTheme(),
    locale: locale,
    localizationsDelegates: const <LocalizationsDelegate<Object?>>[
      AppL10n.delegate,
      GlobalMaterialLocalizations.delegate,
      GlobalWidgetsLocalizations.delegate,
      GlobalCupertinoLocalizations.delegate,
    ],
    supportedLocales: AppL10n.supportedLocales,
    builder: (BuildContext context, Widget? app) =>
        MediaQuery(data: MediaQuery.of(context).copyWith(textScaler: TextScaler.linear(textScale)), child: app!),
    home: Scaffold(body: SingleChildScrollView(child: child)),
  );
}
