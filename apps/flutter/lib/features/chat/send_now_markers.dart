import '../../core/net/bridge_client.dart';

bool isNoSuchOp(BridgeException error) =>
    error.isInvalid && (error.message.contains('lệnh') || error.message.contains('No such op'));
