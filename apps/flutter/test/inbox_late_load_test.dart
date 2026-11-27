import 'dart:async';

import 'package:agent_console/core/net/bridge_client.dart';
import 'package:agent_console/core/pairing/mac_profile.dart';
import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/connection/connection.dart';
import 'package:agent_console/features/inbox/inbox_notifier.dart';
import 'package:agent_console/features/macs/macs_notifier.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/two_computers.dart';

InboxItem blocking(String id) => InboxItem.fromJson(<String, Object?>{
      'id': id,
      'kind': 'consent',
      'title': 'T',
      'detail': '',
      'createdMs': 1,
      'actions': <Object?>[],
      'state': 'open',
      'seen': false,
      'blocking': true,
    });

class _Client implements BridgeClient {
  _Client(this.load);
  Future<List<InboxItem>> Function() load;

  @override
  bool get isConnected => true;

  @override
  Stream<BridgeEvent> get events => const Stream<BridgeEvent>.empty();

  @override
  Future<List<InboxItem>> inboxList() => load();

  @override
  dynamic noSuchMethod(Invocation invocation) => throw UnimplementedError('${invocation.memberName}');
}

class _Macs extends MacsNotifier {
  @override
  MacsState build() => const MacsState(macs: <MacProfile>[macA, macB], activeId: 'aaaaaaaaaaaaaaaa', loaded: true);
  void use(String id) => state = MacsState(macs: state.macs, activeId: id, loaded: true);
}

class _Holder extends Notifier<BridgeClient?> {
  @override
  BridgeClient? build() => null;
  void set(BridgeClient? client) => state = client;
}

final NotifierProvider<_Holder, BridgeClient?> _holder = NotifierProvider<_Holder, BridgeClient?>(_Holder.new);

void main() {
  late ProviderContainer container;

  setUp(() {
    container = ProviderContainer(overrides: [
      macsProvider.overrideWith(_Macs.new),
      bridgeClientProvider.overrideWith((Ref ref) => ref.watch(_holder)),
      connectedCountProvider.overrideWith(() => _Count()),
    ]);
    addTearDown(container.dispose);
    container.listen<InboxState>(inboxProvider, (InboxState? a, InboxState b) {});
  });

  void switchTo(String id, BridgeClient client) {
    (container.read(macsProvider.notifier) as _Macs).use(id);
    container.read(_holder.notifier).set(client);
  }

  test('a late list from the old Mac does not land in the new Mac state', () async {
    final Completer<List<InboxItem>> late = Completer<List<InboxItem>>();
    switchTo('aaaaaaaaaaaaaaaa', _Client(() => late.future));
    await Future<void>.delayed(Duration.zero);
    switchTo('bbbbbbbbbbbbbbbb', _Client(() async => <InboxItem>[]));
    await Future<void>.delayed(Duration.zero);
    expect(container.read(inboxProvider).macId, 'bbbbbbbbbbbbbbbb');
    expect(container.read(inboxProvider).phase, InboxPhase.ready);
    late.complete(<InboxItem>[blocking('from-a')]);
    await Future<void>.delayed(Duration.zero);
    expect(container.read(inboxProvider).items, isEmpty);
  });

  test('a late unsupported answer from the old Mac does not mark the new Mac unsupported', () async {
    final Completer<List<InboxItem>> late = Completer<List<InboxItem>>();
    switchTo('aaaaaaaaaaaaaaaa', _Client(() => late.future));
    await Future<void>.delayed(Duration.zero);
    switchTo('bbbbbbbbbbbbbbbb', _Client(() async => <InboxItem>[blocking('b1')]));
    await Future<void>.delayed(Duration.zero);
    late.completeError(const BridgeException('invalid', ''));
    await Future<void>.delayed(Duration.zero);
    expect(container.read(inboxProvider).phase, InboxPhase.ready);
    expect(container.read(inboxProvider).items.map((InboxItem i) => i.id), <String>['b1']);
  });

  test('the Mac id survives an unsupported answer and a later refresh', () async {
    bool unsupported = true;
    switchTo('aaaaaaaaaaaaaaaa', _Client(() async {
      if (unsupported) throw const BridgeException('invalid', '');
      return <InboxItem>[blocking('a1')];
    }));
    await Future<void>.delayed(Duration.zero);
    expect(container.read(inboxProvider).phase, InboxPhase.unsupported);
    expect(container.read(inboxProvider).macId, 'aaaaaaaaaaaaaaaa');
    unsupported = false;
    await container.read(inboxProvider.notifier).refresh();
    expect(container.read(inboxProvider).phase, InboxPhase.ready);
    expect(container.read(inboxProvider).macId, 'aaaaaaaaaaaaaaaa');
  });

  test('of two overlapping loads on one client the newer one wins even if the older finishes last', () async {
    final List<Completer<List<InboxItem>>> calls = <Completer<List<InboxItem>>>[];
    switchTo('aaaaaaaaaaaaaaaa', _Client(() {
      final Completer<List<InboxItem>> c = Completer<List<InboxItem>>();
      calls.add(c);
      return c.future;
    }));
    await Future<void>.delayed(Duration.zero);
    final Future<void> refreshed = container.read(inboxProvider.notifier).refresh();
    await Future<void>.delayed(Duration.zero);
    expect(calls, hasLength(2));
    calls[1].complete(<InboxItem>[blocking('new')]);
    await refreshed;
    calls[0].complete(<InboxItem>[blocking('old')]);
    await Future<void>.delayed(Duration.zero);
    expect(container.read(inboxProvider).items.map((InboxItem i) => i.id), <String>['new']);
  });

  test('an older load that fails last does not overwrite the newer list', () async {
    final List<Completer<List<InboxItem>>> calls = <Completer<List<InboxItem>>>[];
    switchTo('aaaaaaaaaaaaaaaa', _Client(() {
      final Completer<List<InboxItem>> c = Completer<List<InboxItem>>();
      calls.add(c);
      return c.future;
    }));
    await Future<void>.delayed(Duration.zero);
    final Future<void> refreshed = container.read(inboxProvider.notifier).refresh();
    await Future<void>.delayed(Duration.zero);
    calls[1].complete(<InboxItem>[blocking('new')]);
    await refreshed;
    calls[0].completeError(const BridgeException('invalid', ''));
    await Future<void>.delayed(Duration.zero);
    expect(container.read(inboxProvider).phase, InboxPhase.ready);
    expect(container.read(inboxProvider).items.map((InboxItem i) => i.id), <String>['new']);
  });
}

class _Count extends ConnectedCountNotifier {
  @override
  int build() => 1;
}
