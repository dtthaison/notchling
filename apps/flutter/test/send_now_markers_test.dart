import 'package:agent_console/core/net/bridge_client.dart';
import 'package:agent_console/features/chat/send_now_markers.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('send_now_unsupported_on_either_marker', () {
    expect(isNoSuchOp(const BridgeException('invalid', 'Không có lệnh "session.sendNow".')), isTrue);
    expect(isNoSuchOp(const BridgeException('invalid', 'No such op "session.sendNow".')), isTrue);
    expect(isNoSuchOp(const BridgeException('busy', 'Không có lệnh "session.sendNow".')), isFalse);
    expect(isNoSuchOp(const BridgeException('busy', 'No such op "session.sendNow".')), isFalse);
    expect(isNoSuchOp(const BridgeException('invalid', 'Missing "sessionId".')), isFalse);
  });
}
