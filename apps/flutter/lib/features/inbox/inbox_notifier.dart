import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/net/bridge_client.dart';
import '../../core/protocol/models.dart';
import '../connection/connection.dart';

enum InboxPhase { loading, ready, unsupported }

class InboxState {
  const InboxState({
    this.phase = InboxPhase.loading,
    this.items = const <InboxItem>[],
    this.busy = const <String>{},
    this.errors = const <String, String>{},
    this.declined = const <String>{},
  });

  final InboxPhase phase;
  final List<InboxItem> items;
  final Set<String> busy;
  final Map<String, String> errors;
  final Set<String> declined;

  List<InboxItem> get needsYou => _newestFirst(items.where((InboxItem i) => i.isOpen));
  List<InboxItem> get done => _newestFirst(items.where((InboxItem i) => !i.isOpen));
  int get openCount => items.where((InboxItem i) => i.isOpen).length;

  static List<InboxItem> _newestFirst(Iterable<InboxItem> items) =>
      items.toList()..sort((InboxItem a, InboxItem b) => b.createdMs.compareTo(a.createdMs));

  InboxState copyWith({
    InboxPhase? phase,
    List<InboxItem>? items,
    Set<String>? busy,
    Map<String, String>? errors,
    Set<String>? declined,
  }) => InboxState(
    phase: phase ?? this.phase,
    items: items ?? this.items,
    busy: busy ?? this.busy,
    errors: errors ?? this.errors,
    declined: declined ?? this.declined,
  );
}

final NotifierProvider<InboxNotifier, InboxState> inboxProvider = NotifierProvider<InboxNotifier, InboxState>(InboxNotifier.new);

final Provider<int> inboxOpenCountProvider = Provider<int>((Ref ref) => ref.watch(inboxProvider).openCount);

class InboxNotifier extends Notifier<InboxState> {
  List<InboxItem> _kept = const <InboxItem>[];
  BridgeClient? _keptFor;

  @override
  InboxState build() {
    final BridgeClient? client = ref.watch(bridgeClientProvider);
    ref.watch(connectedCountProvider);
    if (client == null) return const InboxState();
    final List<InboxItem> kept = identical(client, _keptFor) ? _kept : const <InboxItem>[];
    _keptFor = client;

    final StreamSubscription<BridgeEvent> events = client.events.listen((BridgeEvent event) {
      if (event is InboxEvent) _apply(event);
    });
    ref.onDispose(events.cancel);
    if (client.isConnected) unawaited(_load(client));
    return InboxState(phase: kept.isEmpty ? InboxPhase.loading : InboxPhase.ready, items: kept);
  }

  @override
  set state(InboxState value) {
    _kept = value.items;
    super.state = value;
  }

  void _apply(InboxEvent event) {
    final String? removed = event.removedId;
    if (removed != null) {
      state = state.copyWith(items: state.items.where((InboxItem i) => i.id != removed).toList());
      return;
    }
    final InboxItem? item = event.item;
    if (item == null) return;
    final List<InboxItem> items = <InboxItem>[
      for (final InboxItem existing in state.items)
        if (existing.id != item.id) existing,
      item,
    ];
    state = state.copyWith(phase: InboxPhase.ready, items: items);
  }

  Future<void> _load(BridgeClient client) async {
    try {
      final List<InboxItem> items = await client.inboxList();
      if (!ref.mounted) return;
      state = state.copyWith(phase: InboxPhase.ready, items: items);
    } on BridgeException catch (error) {
      if (!ref.mounted) return;
      if (error.code == 'invalid' || error.code == 'not_found') {
        state = const InboxState(phase: InboxPhase.unsupported);
      } else {
        state = state.copyWith(phase: InboxPhase.ready);
      }
    }
  }

  Future<void> refresh() async {
    final BridgeClient? client = ref.read(bridgeClientProvider);
    if (client != null) await _load(client);
  }

  Future<void> answer(InboxItem item, String action) async {
    final BridgeClient? client = ref.read(bridgeClientProvider);
    if (client == null || state.busy.contains(item.id)) return;
    state = state.copyWith(
      busy: <String>{...state.busy, item.id},
      errors: <String, String>{...state.errors}..remove(item.id),
      declined: <String>{...state.declined}..remove(item.id),
    );
    try {
      final InboxAnswer result = await client.inboxAnswer(item.id, action);
      if (!ref.mounted) return;
      if (!result.answered && result.state == 'open') {
        state = state.copyWith(
          busy: <String>{...state.busy}..remove(item.id),
          declined: <String>{...state.declined, item.id},
        );
        return;
      }
      final InboxItem closed = result.answered
          ? item.closed(state: 'answered', answer: action, answeredBy: 'phone')
          : item.closed(state: result.state, answer: result.answer, answeredBy: result.state == 'answered' ? 'mac' : null);
      _finish(item.id, closed);
    } on BridgeException catch (error) {
      if (!ref.mounted) return;
      state = state.copyWith(
        busy: <String>{...state.busy}..remove(item.id),
        errors: <String, String>{...state.errors, item.id: error.message},
      );
    }
  }

  void _finish(String id, InboxItem closed) {
    state = state.copyWith(
      busy: <String>{...state.busy}..remove(id),
      items: <InboxItem>[
        for (final InboxItem existing in state.items)
          if (existing.id == id) closed else existing,
      ],
    );
  }
}
