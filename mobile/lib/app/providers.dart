import 'dart:io';

import 'package:connectivity_plus/connectivity_plus.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../core/net/ws_transport.dart';
import '../core/pairing/mac_store.dart';

/// Where tokens live. Overridden with memory in tests.
final Provider<SecretStore> secretStoreProvider = Provider<SecretStore>((Ref ref) => const KeychainSecretStore());

/// How sockets are opened. Overridden with a fake in tests.
final Provider<WsConnector> wsConnectorProvider = Provider<WsConnector>((Ref ref) => connectEndpoint);

/// Fires when the phone's network changes (Wi-Fi to mobile data and back). Overridden in tests.
final Provider<Stream<void>> networkChangesProvider =
    Provider<Stream<void>>((Ref ref) => Connectivity().onConnectivityChanged.map((List<ConnectivityResult> _) {}));

/// Shown on the Mac when it asks whether to allow this phone.
final Provider<String> deviceNameProvider = Provider<String>((Ref ref) => Platform.isAndroid ? 'Android' : 'iPhone');

final Provider<String> platformNameProvider = Provider<String>((Ref ref) => Platform.isAndroid ? 'android' : 'ios');
