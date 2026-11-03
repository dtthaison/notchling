import 'package:agent_console/app/alerts.dart';
import 'package:agent_console/app/providers.dart';
import 'package:agent_console/app/shell.dart';
import 'dart:io';

import 'package:agent_console/core/net/ws_transport.dart';
import 'package:agent_console/core/pairing/mac_profile.dart';
import 'package:agent_console/core/pairing/mac_store.dart';
import 'package:agent_console/core/protocol/models.dart';
import 'package:agent_console/features/sessions/status_style.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:flutter_test/flutter_test.dart';

import 'support/harness.dart';

Session _s(String id, SessionState state) => Session(
      id: id,
      name: 'Session $id',
      project: 'p',
      cwd: '/p',
      entrypoint: 'cli',
      state: state,
      control: SessionControl.console,
      lastActivity: DateTime(2026, 11, 3),
    );

class _FakeKeeper implements BackgroundKeeper {
  final List<String> calls = <String>[];
  @override
  Future<void> start(String text) async => calls.add('start:$text');
  @override
  Future<void> stop() async => calls.add('stop');
}

void main() {
  group('alerts between lists', () {
    test('needs-input and finished are each raised once per change', () {
      final Map<String, AlertKind> announced = <String, AlertKind>{};
      final List<Session> running = <Session>[_s('a', SessionState.running), _s('b', SessionState.running)];
      final List<Session> next = <Session>[_s('a', SessionState.needsInput), _s('b', SessionState.done)];
      final Map<String, AlertKind> first = alertsBetween(running, next, announced);
      expect(first, <String, AlertKind>{'a': AlertKind.needsInput, 'b': AlertKind.finished});
      expect(alertsBetween(next, next, announced), isEmpty, reason: 'the same list again says nothing new');
      // A poll that flickers back and forth between two states does not repeat the alert for the same change.
      expect(alertsBetween(running, next, announced), isEmpty);
    });

    test('after the session moves on, its next change is announced again', () {
      final Map<String, AlertKind> announced = <String, AlertKind>{};
      final List<Session> running = <Session>[_s('a', SessionState.running)];
      final List<Session> done = <Session>[_s('a', SessionState.done)];
      expect(alertsBetween(running, done, announced), <String, AlertKind>{'a': AlertKind.finished});
      expect(alertsBetween(done, running, announced), isEmpty);
      expect(alertsBetween(running, done, announced), <String, AlertKind>{'a': AlertKind.finished});
    });

    test('a session first seen already done or waiting is not announced, and idle to done is not "finished"', () {
      final Map<String, AlertKind> announced = <String, AlertKind>{};
      expect(alertsBetween(<Session>[], <Session>[_s('x', SessionState.needsInput), _s('y', SessionState.done)], announced), isEmpty);
      expect(alertsBetween(<Session>[_s('z', SessionState.exited)], <Session>[_s('z', SessionState.done)], announced), isEmpty);
    });
  });

  group('prefs', () {
    ProviderContainer container(MemorySecretStore store) {
      final ProviderContainer c = ProviderContainer(overrides: <Override>[secretStoreProvider.overrideWithValue(store)]);
      addTearDown(c.dispose);
      c.listen<AlertPrefs>(alertPrefsProvider, (AlertPrefs? a, AlertPrefs b) {});
      return c;
    }

    test('the background setting starts undecided and is remembered', () async {
      final MemorySecretStore store = MemorySecretStore();
      final ProviderContainer first = container(store);
      await until(() => first.read(alertPrefsProvider).loaded);
      expect(first.read(alertPrefsProvider).keepAlive, isNull);
      await first.read(alertPrefsProvider.notifier).setKeepAlive(on: true);

      final ProviderContainer second = container(store);
      await until(() => second.read(alertPrefsProvider).loaded);
      expect(second.read(alertPrefsProvider).keepAlive, isTrue);
    });

    test('muting a session is per session and survives a restart', () async {
      final MemorySecretStore store = MemorySecretStore();
      final ProviderContainer first = container(store);
      await until(() => first.read(alertPrefsProvider).loaded);
      await first.read(alertPrefsProvider.notifier).setMuted('a', muted: true);
      await first.read(alertPrefsProvider.notifier).setMuted('b', muted: true);
      await first.read(alertPrefsProvider.notifier).setMuted('b', muted: false);

      final ProviderContainer second = container(store);
      await until(() => second.read(alertPrefsProvider).loaded);
      expect(second.read(alertPrefsProvider).muted, <String>{'a'});
    });
  });

  group('foreground service', () {
    ProviderContainer container(_FakeKeeper service, {required bool paired}) {
      final MemorySecretStore store = MemorySecretStore()..values['prefs.keepalive'] = 'on';
      if (paired) {
        store.values['macs.v1'] = encodeProfiles(<MacProfile>[testMac]);
        store.values['macs.active'] = 'm';
      }
      final ProviderContainer c = ProviderContainer(overrides: <Override>[
        secretStoreProvider.overrideWithValue(store),
        keepAliveProvider.overrideWithValue(service),
        wsConnectorProvider.overrideWithValue((Endpoint e) async => throw const SocketException('offline')),
        networkChangesProvider.overrideWithValue(const Stream<void>.empty()),
      ]);
      addTearDown(c.dispose);
      c.listen<void>(keepAliveControllerProvider, (void a, void b) {});
      return c;
    }

    test('runs while the setting is on and a Mac is paired, and stops when it is switched off', () async {
      final _FakeKeeper service = _FakeKeeper();
      final ProviderContainer c = container(service, paired: true);
      await until(() => service.calls.any((String x) => x.startsWith('start')));
      expect(service.calls.firstWhere((String x) => x.startsWith('start')), contains('Agent Console đang theo dõi'));
      await c.read(alertPrefsProvider.notifier).setKeepAlive(on: false);
      await until(() => service.calls.contains('stop'));
    });

    test('does nothing without a paired Mac', () async {
      final _FakeKeeper service = _FakeKeeper();
      final ProviderContainer c = container(service, paired: false);
      await until(() => c.read(alertPrefsProvider).loaded);
      await Future<void>.delayed(const Duration(milliseconds: 30));
      expect(service.calls.where((String x) => x.startsWith('start')), isEmpty);
    });
  });

  group('breathing dots and idle frames', () {
    testWidgets('a hidden tab (TickerMode off) does not use the shared clock', (WidgetTester tester) async {
      await tester.pumpWidget(host(Column(children: const <Widget>[
        TickerMode(enabled: false, child: StatusDot(state: SessionState.running)),
      ])));
      expect(tester.hasRunningAnimations, isFalse);
      // Nothing scheduled: pumping a few seconds draws no extra frames for the hidden dot.
      int frames = 0;
      tester.binding.addPersistentFrameCallback((Duration _) => frames++);
      await tester.pump(const Duration(seconds: 2));
      expect(frames, lessThanOrEqualTo(1));
    });

    testWidgets('a visible running dot ticks on the shared clock and stops when it leaves', (WidgetTester tester) async {
      await tester.pumpWidget(host(const StatusDot(state: SessionState.running)));
      final int before = BreathClock.instance.phase;
      await tester.pump(const Duration(milliseconds: 450));
      expect(BreathClock.instance.phase, isNot(before));
      await tester.pumpWidget(host(const SizedBox()));
      final int after = BreathClock.instance.phase;
      await tester.pump(const Duration(seconds: 1));
      expect(BreathClock.instance.phase, after, reason: 'no dot left, the clock stopped');
    });

    testWidgets('after 20 s without a touch the dots rest and draw nothing; a touch wakes them', (WidgetTester tester) async {
      DateTime clock = DateTime(2026, 11, 3, 12);
      BreathClock.instance.now = () => clock;
      addTearDown(() => BreathClock.instance.now = DateTime.now);
      await tester.pumpWidget(host(const StatusDot(state: SessionState.running)));
      await tester.pump(const Duration(milliseconds: 450));
      final int moving = BreathClock.instance.phase;
      expect(moving, isNot(0));

      clock = clock.add(const Duration(seconds: 25));
      await tester.pump(const Duration(milliseconds: 250));
      await tester.pump(const Duration(milliseconds: 250));
      final int rested = BreathClock.instance.phase;
      expect(rested, 0, reason: 'rests on a full dot');
      int frames = 0;
      tester.binding.addPersistentFrameCallback((Duration _) => frames++);
      await tester.pump(const Duration(seconds: 3));
      expect(frames, lessThanOrEqualTo(1), reason: 'nothing animates while resting');

      final TestGesture touch = await tester.startGesture(const Offset(5, 5));
      await touch.up();
      await tester.pump(const Duration(milliseconds: 450));
      expect(BreathClock.instance.phase, isNot(0), reason: 'a touch wakes the breathing');
    });

    testWidgets('several dots share one clock', (WidgetTester tester) async {
      await tester.pumpWidget(host(Column(children: const <Widget>[
        StatusDot(state: SessionState.running),
        StatusDot(state: SessionState.running),
        StatusDot(state: SessionState.running),
      ])));
      int notifications = 0;
      void count() => notifications++;
      BreathClock.instance.addListener(count);
      await tester.pump(const Duration(milliseconds: 1000));
      BreathClock.instance.removeListener(count);
      expect(notifications, lessThanOrEqualTo(6), reason: 'about five steps a second whatever the number of dots');
    });
  });

  group('KeyboardLift', () {
    testWidgets('moves the composer with the inset without resizing the list, then makes room once', (WidgetTester tester) async {
      final ValueNotifier<double> inset = ValueNotifier<double>(0);
      final KeyboardLiftController lift = KeyboardLiftController();
      addTearDown(inset.dispose);
      addTearDown(lift.dispose);
      int listBuilds = 0;
      // Created once, as the chat screen does: its parent does not rebuild with the keyboard.
      final Widget list = ValueListenableBuilder<double>(
        valueListenable: lift.settled,
        builder: (BuildContext context, double room, Widget? _) {
          listBuilds++;
          return Container(key: const Key('list'), color: Colors.blue, padding: EdgeInsets.only(bottom: room));
        },
      );
      final Widget composerWidget = Container(key: const Key('composer'), height: 50, color: Colors.red);
      await tester.pumpWidget(MaterialApp(
        home: ValueListenableBuilder<double>(
          valueListenable: inset,
          builder: (BuildContext context, double value, Widget? child) => MediaQuery(
            data: MediaQuery.of(context).copyWith(viewInsets: EdgeInsets.only(bottom: value)),
            child: Column(children: <Widget>[
              Expanded(
                child: KeyboardLift(
                  controller: lift,
                  list: list,
                  composer: composerWidget,
                ),
              ),
              const SizedBox(height: 80), // the tab bar below the page
            ]),
          ),
        ),
      ));
      await tester.pump();
      await tester.pump();
      final double composerRest = tester.getBottomLeft(find.byKey(const Key('composer'))).dy;
      final Size listSize = tester.getSize(find.byKey(const Key('list')));
      final int buildsAtRest = listBuilds;

      for (final double value in <double>[40, 80, 200, 336]) {
        inset.value = value;
        await tester.pump(const Duration(milliseconds: 8));
        expect(composerRest - tester.getBottomLeft(find.byKey(const Key('composer'))).dy, closeTo(value > 80 ? value - 80 : 0, 0.5), reason: 'inset $value');
        expect(tester.getSize(find.byKey(const Key('list'))), listSize, reason: 'the list is not resized while the keyboard moves');
        expect(listBuilds, buildsAtRest, reason: 'and not rebuilt');
      }
      expect(lift.settled.value, 0, reason: 'still moving');

      await tester.pump(const Duration(milliseconds: 120));
      expect(lift.settled.value, 256, reason: 'settled: room for the keyboard made once');
      await tester.pump();
      expect(composerRest - tester.getBottomLeft(find.byKey(const Key('composer'))).dy, closeTo(256, 0.5), reason: 'the composer does not move at settle');

      inset.value = 0;
      await tester.pump();
      await tester.pump();
      expect(lift.settled.value, 0, reason: 'closed: the room is given back');
      expect(composerRest - tester.getBottomLeft(find.byKey(const Key('composer'))).dy, closeTo(0, 0.5));
    });
  });
}
