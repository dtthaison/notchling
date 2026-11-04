import 'dart:async';
import 'dart:io' show Platform;
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../core/net/bridge_client.dart';
import '../core/net/ws_transport.dart';
import '../features/chat/chat_screen.dart';
import '../features/connection/connection.dart';
import '../features/connection/subscription_sync.dart';
import '../features/macs/macs_notifier.dart';
import '../features/macs/macs_screen.dart';
import '../features/pairing/pairing_screen.dart';
import '../features/sessions/sessions_notifier.dart';
import '../features/sessions/sessions_screen.dart';
import '../features/settings/settings_screen.dart';
import '../features/visualize/visualize_screen.dart';
import '../l10n/app_localizations.dart';
import 'alerts.dart';
import 'nav_state.dart';
import 'notifications.dart';
import 'theme/tokens.dart';
import 'widgets/common.dart';
import 'widgets/alert_banner.dart';
import 'widgets/glass_nav.dart';
import 'widgets/kit.dart';
import 'widgets/motion.dart';

/// Root: pair first, then the three tabs (or three panels from 840 dp, like the Mac).
class AppRoot extends ConsumerStatefulWidget {
  const AppRoot({super.key});

  @override
  ConsumerState<AppRoot> createState() => _AppRootState();
}

class _AppRootState extends ConsumerState<AppRoot> with WidgetsBindingObserver {
  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    unawaited(ref.read(notificationServiceProvider).init());
    ref.read(notificationServiceProvider).taps.listen((String sessionId) {
      ref.read(selectedSessionIdProvider.notifier).select(sessionId);
      ref.read(activeTabProvider.notifier).show(1);
    });
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  /// The first time a Mac is connected on Android: offer to keep the connection alive in the background.
  Future<void> _askKeepAliveOnce() async {
    if (!Platform.isAndroid || !mounted) return;
    // Let the stored choice load first.
    for (int i = 0; i < 20 && !ref.read(alertPrefsProvider).loaded; i++) {
      await Future<void>.delayed(const Duration(milliseconds: 100));
    }
    if (!mounted || ref.read(alertPrefsProvider).keepAlive != null) return;
    final AppL10n l10n = AppL10n.of(context);
    final bool? on = await showDialog<bool>(
      context: context,
      builder: (BuildContext context) => AlertDialog(
        title: Text(l10n.keepAliveTitle),
        content: Text(l10n.keepAliveBody),
        actions: <Widget>[
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: Text(l10n.notNow),
          ),
          TextButton(
            onPressed: () => Navigator.pop(context, true),
            child: Text(l10n.keepAliveEnable),
          ),
        ],
      ),
    );
    if (on != null) {
      await ref.read(alertPrefsProvider.notifier).setKeepAlive(on: on);
    }
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) =>
      ref.read(lifecycleProvider.notifier).set(state);

  @override
  Widget build(BuildContext context) {
    ref.watch(attentionWatcherProvider);
    ref.watch(keepAliveControllerProvider);
    ref.listen<int>(connectedCountProvider, (int? previous, int next) {
      if (next > 0 && (previous ?? 0) == 0) {
        unawaited(ref.read(notificationServiceProvider).requestPermission());
        unawaited(_askKeepAliveOnce());
      }
    });
    final MacsState macs = ref.watch(macsProvider);
    if (!macs.loaded) {
      return const Scaffold(body: SafeArea(child: SkeletonRows()));
    }
    if (macs.active == null) return const PairingScreen();
    ref.watch(subscriptionSyncProvider);
    return const HomeShell();
  }
}

class HomeShell extends ConsumerStatefulWidget {
  const HomeShell({super.key});

  @override
  ConsumerState<HomeShell> createState() => _HomeShellState();
}

class _HomeShellState extends ConsumerState<HomeShell> {
  StreamSubscription<BridgeEvent>? _events;

  @override
  void initState() {
    super.initState();
    _bind(ref.read(bridgeClientProvider));
  }

  /// `{"event":"open"}` from the Mac ("Open Agent Console" in Notchling): show the Session tab, but only
  /// when the app is in front.
  void _bind(BridgeClient? client) {
    unawaited(_events?.cancel());
    _events = client?.events.listen((BridgeEvent event) {
      if (event is OpenEvent &&
          ref.read(lifecycleProvider) == AppLifecycleState.resumed) {
        showSessionsTab(ref);
      }
    });
  }

  @override
  void dispose() {
    unawaited(_events?.cancel());
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    ref.listen<BridgeClient?>(
      bridgeClientProvider,
      (BridgeClient? previous, BridgeClient? next) => _bind(next),
    );
    final int tab = ref.watch(activeTabProvider);
    final bool hideBar =
        tab == 1 &&
        ref.watch(selectedSessionIdProvider) != null &&
        MediaQuery.sizeOf(context).width < Metric.threePanelBreakpoint;

    final bool wide =
        MediaQuery.sizeOf(context).width >= Metric.threePanelBreakpoint;
    final Kit kit = Kit.of(context);

    Widget inset(Widget child, {bool bottom = false}) {
      final EdgeInsets p = MediaQuery.paddingOf(context);
      return MediaQuery.removePadding(
        context: context,
        removeTop: true,
        removeBottom: bottom,
        child: Padding(
          padding: EdgeInsets.only(top: p.top, bottom: bottom ? p.bottom : 0),
          child: child,
        ),
      );
    }

    return Scaffold(
      // The keyboard must not resize the whole page each frame of its animation (all three tabs would relayout);
      // the chat composer alone follows the inset (KeyboardInset) and the tab bar steps aside while it is up.
      resizeToAvoidBottomInset: false,
      // The bars are frosted glass over the content: the lists scroll under them (their padding comes from the
      // Scaffold's MediaQuery), so nothing needs an opaque strip behind it.
      extendBody: !wide,
      extendBodyBehindAppBar: true,
      // The Chat tab with a session open has its own header (with the Macs button): a second bar above it would eat a
      // quarter of the screen.
      appBar: hideBar
          ? null
          : AppBar(
              toolbarHeight: 48,
              backgroundColor: Colors.transparent,
              elevation: 0,
              scrolledUnderElevation: 0,
              surfaceTintColor: Colors.transparent,
              flexibleSpace: const GlassBar(
                top: true,
                child: SizedBox.expand(),
              ),
              title: Text(l10n.appName, style: kit.titleMd),
              actions: <Widget>[
                IconButton(
                  tooltip: l10n.openMacs,
                  icon: const Icon(Icons.laptop_mac),
                  constraints: const BoxConstraints(
                    minWidth: Metric.minTarget,
                    minHeight: Metric.minTarget,
                  ),
                  onPressed: () => unawaited(
                    Navigator.of(context).push(
                      MaterialPageRoute<void>(
                        builder: (BuildContext context) => const MacsScreen(),
                      ),
                    ),
                  ),
                ),
                IconButton(
                  tooltip: l10n.settingsTitle,
                  icon: const Icon(Icons.settings_outlined),
                  constraints: const BoxConstraints(
                    minWidth: Metric.minTarget,
                    minHeight: Metric.minTarget,
                  ),
                  onPressed: () => unawaited(
                    Navigator.of(context).push(
                      MaterialPageRoute<void>(
                        builder: (BuildContext context) =>
                            const SettingsScreen(),
                      ),
                    ),
                  ),
                ),
              ],
            ),
      body: wide
          ? SafeArea(
              bottom: false,
              child: Column(
                children: <Widget>[
                  const ConnectionBanner(),
                  Expanded(
                    child: Row(
                      children: <Widget>[
                        SizedBox(
                          width: 360,
                          child: SessionsScreen(onOpenChat: () {}),
                        ),
                        const VerticalDivider(),
                        const Expanded(child: ChatScreen()),
                        const VerticalDivider(),
                        const Expanded(child: VisualizeScreen()),
                      ],
                    ),
                  ),
                ],
              ),
            )
          : Stack(
              children: <Widget>[
                Positioned.fill(
                  child: _TabFade(
                    index: tab,
                    child: IndexedStack(
                      index: tab,
                      children: <Widget>[
                        _TabSlot(
                          active: tab == 0,
                          child: SessionsScreen(
                            onOpenChat: () =>
                                ref.read(activeTabProvider.notifier).show(1),
                          ),
                        ),
                        _TabSlot(
                          active: tab == 1,
                          child: inset(const ChatScreen()),
                        ),
                        // Only built while shown, so the Phone stream and viz loads stop when you leave.
                        if (tab == 2)
                          inset(const VisualizeScreen(), bottom: true)
                        else
                          const SizedBox.shrink(),
                      ],
                    ),
                  ),
                ),
                Positioned(
                  top: MediaQuery.paddingOf(context).top,
                  left: 0,
                  right: 0,
                  child: const ConnectionBanner(),
                ),
                Positioned(
                  top: MediaQuery.paddingOf(context).top,
                  left: 0,
                  right: 0,
                  child: const InAppAlertBanner(),
                ),
              ],
            ),
      bottomNavigationBar: const _BottomNav(),
    );
  }
}

/// Đang kết nối / Mất kết nối, thử lại sau Ns / Đã kết nối.
class ConnectionBanner extends ConsumerStatefulWidget {
  const ConnectionBanner({super.key});

  @override
  ConsumerState<ConnectionBanner> createState() => _ConnectionBannerState();
}

class _ConnectionBannerState extends ConsumerState<ConnectionBanner> {
  Timer? _tick;
  Timer? _hideConnected;
  bool _showConnected = false;
  LinkPhase? _last;

  @override
  void initState() {
    super.initState();
    // Only the retry countdown needs a once-a-second redraw; a connected, idle app asks for no frames.
    _tick = Timer.periodic(const Duration(seconds: 1), (Timer _) {
      if (mounted &&
          ref.read(connectionStatusProvider).phase == LinkPhase.disconnected) {
        setState(() {});
      }
    });
  }

  @override
  void dispose() {
    _tick?.cancel();
    _hideConnected?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final AppL10n l10n = AppL10n.of(context);
    final ConnectionStatus status = ref.watch(connectionStatusProvider);
    if (_last != status.phase) {
      if (status.phase == LinkPhase.connected && _last != null) {
        _showConnected = true;
        _hideConnected?.cancel();
        _hideConnected = Timer(const Duration(seconds: 2), () {
          if (mounted) setState(() => _showConnected = false);
        });
      }
      _last = status.phase;
    }

    String? text;
    Color color = Kit.of(context).textMuted;
    IconData icon = Icons.sync;
    VoidCallback? retry;
    switch (status.phase) {
      case LinkPhase.connecting:
        text = l10n.connecting;
      case LinkPhase.disconnected:
        if (status.revoked) {
          text = l10n.connectionRevoked;
          color = Kit.of(context).danger;
          icon = Icons.link_off;
        } else {
          final int seconds = status.retryAt == null
              ? 0
              : status.retryAt!
                    .difference(DateTime.now())
                    .inSeconds
                    .clamp(0, 999);
          text = status.error != null
              ? l10n.noRouteToMac
              : l10n.connectionLostRetry(seconds);
          color = Kit.of(context).danger;
          icon = Icons.wifi_off;
          retry = () => ref.read(bridgeClientProvider)?.retryNow();
        }
      case LinkPhase.connected:
        final bool viaInternet = status.route == RouteKind.tunnel;
        // The home network is the normal case and fades after two seconds; the internet route stays visible.
        if (_showConnected || viaInternet) {
          final String route = viaInternet
              ? l10n.routeInternet
              : l10n.routeHome;
          text = status.route == null
              ? l10n.connected
              : l10n.connectedVia(route);
          color = viaInternet ? Kit.of(context).needs : Kit.of(context).working;
          icon = viaInternet ? Icons.public : Icons.check;
        }
    }
    if (text == null) return const SizedBox.shrink();
    return Semantics(
      liveRegion: true,
      label: text,
      child: Container(
        width: double.infinity,
        padding: const EdgeInsets.symmetric(
          horizontal: Space.lg,
          vertical: Space.sm,
        ),
        decoration: BoxDecoration(
          color: Kit.of(context).sidebar,
          border: Border(bottom: BorderSide(color: Kit.of(context).hairline)),
        ),
        child: Row(
          children: <Widget>[
            Icon(icon, size: 14, color: color),
            const SizedBox(width: Space.sm),
            Expanded(
              child: Text(
                text,
                maxLines: 3,
                overflow: TextOverflow.ellipsis,
                style: Kit.of(
                  context,
                ).labelSm.copyWith(color: color, fontSize: 12),
              ),
            ),
            if (retry != null)
              TextButton(onPressed: retry, child: Text(l10n.retryNow)),
          ],
        ),
      ),
    );
  }
}

/// The tab bar. Hidden only on wide layouts. It stays laid out while the keyboard is up (the keyboard covers
/// it), so nothing on the page moves when the keyboard starts to slide.
class _BottomNav extends ConsumerWidget {
  const _BottomNav();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    if (MediaQuery.sizeOf(context).width >= Metric.threePanelBreakpoint) {
      return const SizedBox.shrink();
    }
    return GlassNav(
      selected: ref.watch(activeTabProvider),
      // Chat is the tab whose list moves under the bar with the keyboard: no blur there.
      solid: ref.watch(activeTabProvider) == 1,
      onSelect: (int i) => ref.read(activeTabProvider.notifier).show(i),
    );
  }
}

/// Shared between [KeyboardLift] and the list inside it: how much of the keyboard's height the list has already
/// made room for in its real layout. It changes once per keyboard opening or closing, not per frame.
class KeyboardLiftController {
  final ValueNotifier<double> settled = ValueNotifier<double>(0);

  void dispose() => settled.dispose();
}

/// The chat's message list with the composer under it, rising with the keyboard.
///
/// While the keyboard slides (a new inset about every 8 ms), the list keeps its size and nothing is laid out
/// again: the composer is moved up with a paint-only transform and the list content moves with it, clipped to the
/// list's own area. Once the inset has stopped changing, the room is made in the list's real layout in one step
/// (bottom padding equal to the keyboard's height) and the transform on the list drops to zero in the same frame,
/// so nothing visibly changes at that moment. Closing does the same in reverse. Resizing the list on every step
/// made the list lay out its visible messages every frame, which showed as a stutter (the Sessions search field,
/// which never moves, stayed smooth).
///
/// The same widgets at every inset, so the composer keeps the focus that opened the keyboard. A transform keeps
/// hit testing correct. [composer] should include anything that rises with it (a pending-prompt card).
class KeyboardLift extends StatefulWidget {
  const KeyboardLift({
    super.key,
    required this.controller,
    required this.list,
    required this.composer,
  });
  final KeyboardLiftController controller;
  final Widget list;
  final Widget composer;

  @override
  State<KeyboardLift> createState() => _KeyboardLiftState();
}

class _KeyboardLiftState extends State<KeyboardLift> {
  /// How long the inset must hold still before the list's layout is updated.
  static const Duration settleAfter = Duration(milliseconds: 90);

  double _below = 0;
  double _lastLift = 0;
  Timer? _settle;

  @override
  void initState() {
    super.initState();
    widget.controller.settled.addListener(_onSettled);
  }

  @override
  void didUpdateWidget(KeyboardLift oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.controller != widget.controller) {
      oldWidget.controller.settled.removeListener(_onSettled);
      widget.controller.settled.addListener(_onSettled);
    }
  }

  @override
  void dispose() {
    _settle?.cancel();
    widget.controller.settled.removeListener(_onSettled);
    super.dispose();
  }

  void _onSettled() => setState(() {});

  void _measure() {
    final RenderObject? box = context.findRenderObject();
    if (!mounted || box is! RenderBox || !box.hasSize) return;
    final double below =
        MediaQuery.sizeOf(context).height -
        box.localToGlobal(Offset(0, box.size.height)).dy;
    // Measured only while the keyboard is down: with it up the page is covered, not moved, so it stays right.
    if (MediaQuery.viewInsetsOf(context).bottom == 0 &&
        (below - _below).abs() > 0.5) {
      setState(() => _below = below);
    }
  }

  @override
  Widget build(BuildContext context) {
    final double inset = MediaQuery.viewInsetsOf(context).bottom;
    final double lift = math.max(0, inset - _below);
    if (inset == 0) {
      WidgetsBinding.instance.addPostFrameCallback((Duration _) => _measure());
    }
    if (lift != _lastLift) {
      final bool closing = lift < _lastLift;
      _lastLift = lift;
      _settle?.cancel();
      if (closing && lift > 0) {
        // Closing: the list's extra room goes at once and the transform alone carries the content down with the
        // keyboard. Keeping the room while the transform moved the list down left an empty band (the window colour)
        // at the top of the list for the whole animation, about half the screen.
        WidgetsBinding.instance.addPostFrameCallback((Duration _) {
          if (_lastLift > 0 && widget.controller.settled.value != 0) widget.controller.settled.value = 0;
        });
      } else if (lift == 0) {
        // Closed: drop the list's extra room right away; the transform it replaces is already at zero.
        WidgetsBinding.instance.addPostFrameCallback((Duration _) {
          if (_lastLift == 0) widget.controller.settled.value = 0;
        });
      } else {
        _settle = Timer(
          settleAfter,
          () => widget.controller.settled.value = _lastLift,
        );
      }
    }
    final double settled = widget.controller.settled.value;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        Expanded(
          child: ClipRect(
            child: Transform.translate(
              offset: Offset(0, settled - lift),
              child: widget.list,
            ),
          ),
        ),
        Transform.translate(offset: Offset(0, -lift), child: widget.composer),
      ],
    );
  }
}

/// One tab of the stack. A hidden tab stays mounted (scroll position, drafts) but is cut off from MediaQuery
/// changes: while the keyboard animates, every frame brings new view insets, and without this the hidden Sessions
/// tab rebuilt and re-laid out its search field on each one (about 6 ms of every frame, measured on a Redmi).
class _TabSlot extends StatefulWidget {
  const _TabSlot({required this.active, required this.child});
  final bool active;
  final Widget child;

  @override
  State<_TabSlot> createState() => _TabSlotState();
}

class _TabSlotState extends State<_TabSlot> {
  MediaQueryData? _frozen;

  @override
  Widget build(BuildContext context) {
    final MediaQueryData live = MediaQuery.of(context);
    if (widget.active) _frozen = live;
    // TickerMode off for a hidden tab: its animations (the breathing dots) stop instead of keeping frames going.
    return TickerMode(
      enabled: widget.active,
      child: MediaQuery(
        data: widget.active ? live : (_frozen ?? live),
        child: widget.child,
      ),
    );
  }
}

/// A short fade when the tab changes, so switching tabs reads as one gesture. Opacity only (a layer property): the
/// tabs themselves do not rebuild. Reduce Motion skips it.
class _TabFade extends StatefulWidget {
  const _TabFade({required this.index, required this.child});
  final int index;
  final Widget child;

  @override
  State<_TabFade> createState() => _TabFadeState();
}

class _TabFadeState extends State<_TabFade>
    with SingleTickerProviderStateMixin {
  late final AnimationController _controller = AnimationController(
    vsync: this,
    duration: Motion.ui,
    value: 1,
  );
  late final CurvedAnimation _fade = CurvedAnimation(
    parent: _controller,
    curve: Motion.out,
  );

  @override
  void didUpdateWidget(_TabFade oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.index != widget.index &&
        !MediaQuery.disableAnimationsOf(context)) {
      _controller.forward(from: 0.25);
    }
  }

  @override
  void dispose() {
    _fade.dispose();
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) =>
      FadeTransition(opacity: _fade, child: widget.child);
}
