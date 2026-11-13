import 'dart:io';

import 'package:connectivity_plus/connectivity_plus.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../core/net/ws_transport.dart';
import '../core/pairing/mac_store.dart';

final Provider<SecretStore> secretStoreProvider = Provider<SecretStore>((Ref ref) => const KeychainSecretStore());

final Provider<WsConnector> wsConnectorProvider = Provider<WsConnector>((Ref ref) => connectEndpoint);

final Provider<Stream<void>> networkChangesProvider =
    Provider<Stream<void>>((Ref ref) => Connectivity().onConnectivityChanged.map((List<ConnectivityResult> _) {}));

final Provider<String> deviceNameProvider = Provider<String>((Ref ref) => Platform.isAndroid ? 'Android' : 'iPhone');

final Provider<String> platformNameProvider = Provider<String>((Ref ref) => Platform.isAndroid ? 'android' : 'ios');
