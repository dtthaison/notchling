import '../../core/protocol/models.dart';

const int stillWaitingMs = 180000;
const int lastCallLeadMs = 60000;

enum InboxRung { first, stronger, lastCall }

class InboxAlert {
  const InboxAlert({required this.item, required this.rung});

  final InboxItem item;
  final InboxRung rung;
}

abstract class InboxAlertSink {
  Future<void> showInboxAlert(InboxAlert alert);
  Future<void> cancelInboxAlert(String itemId);
}

class _Track {
  _Track(this.firstSeenMs);

  final int firstSeenMs;
  final Set<InboxRung> posted = <InboxRung>{};
}

class InboxAlerter {
  InboxAlerter({required this.sink, required this.nowMs});

  final InboxAlertSink sink;
  final int Function() nowMs;
  final Map<String, _Track> _tracked = <String, _Track>{};

  Future<void> _tail = Future<void>.value();

  Future<void> sync(List<InboxItem> items) {
    final Future<void> run = _tail.then((_) => _run(items));
    _tail = run.then((_) {}, onError: (Object _) {});
    return run;
  }

  Future<void> _run(List<InboxItem> items) async {
    final int now = nowMs();
    final Set<String> live = <String>{};
    final List<InboxAlert> due = <InboxAlert>[];
    for (final InboxItem item in items) {
      if (!item.isBlocking) continue;
      live.add(item.id);
      final _Track track = _tracked.putIfAbsent(item.id, () => _Track(now));
      final InboxRung? rung = _dueRung(item, track, now);
      if (rung != null) due.add(InboxAlert(item: item, rung: rung));
    }
    for (final InboxAlert alert in due) {
      await sink.showInboxAlert(alert);
    }
    final List<String> gone = <String>[
      for (final String id in _tracked.keys)
        if (!live.contains(id)) id,
    ];
    for (final String id in gone) {
      _tracked.remove(id);
      await sink.cancelInboxAlert(id);
    }
  }

  InboxRung? _dueRung(InboxItem item, _Track track, int now) {
    final int start = item.waitingSince ?? track.firstSeenMs;
    final int? end = item.expiresMs;
    final Map<InboxRung, int> times = <InboxRung, int>{
      InboxRung.first: start,
      if (end == null || start + stillWaitingMs < end) InboxRung.stronger: start + stillWaitingMs,
      if (end != null) InboxRung.lastCall: end - lastCallLeadMs,
    };
    InboxRung? latest;
    for (final MapEntry<InboxRung, int> entry in times.entries) {
      if (now < entry.value) continue;
      if (entry.key == InboxRung.lastCall && now >= end!) continue;
      if (track.posted.add(entry.key) && (latest == null || entry.value >= times[latest]!)) latest = entry.key;
    }
    return latest;
  }
}

int inboxMinutesWaiting(InboxItem item, int nowMs) {
  final int since = item.waitingSince ?? item.createdMs;
  final int ms = nowMs - since;
  return ms <= 0 ? 0 : ms ~/ 60000;
}
