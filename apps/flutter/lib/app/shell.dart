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
import '../features/inbox/inbox_notifier.dart';
import '../features/inbox/inbox_screen.dart';
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
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  Future<void> _askKeepAliveOnce() async {
    if (!Platform.isAndroid || !mounted) return;
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
    ref.watch(notificationTapsProvider);
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
      return Builder(
        builder: (BuildContext inner) {
          final EdgeInsets p = MediaQuery.paddingOf(inner);
          return MediaQuery.removePadding(
            context: inner,
            removeTop: true,
            removeBottom: bottom,
            child: Padding(
              padding: EdgeInsets.only(top: p.top, bottom: bottom ? p.bottom : 0),
              child: child,
            ),
          );
        },
      );
    }

    return Scaffold(
      resizeToAvoidBottomInset: false,
      extendBody: !wide,
      extendBodyBehindAppBar: true,
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
                if (wide)
                  IconButton(
                    tooltip: l10n.tabInbox,
                    icon: Badge(
                      isLabelVisible: ref.watch(inboxOpenCountProvider) > 0,
                      label: Text('${ref.watch(inboxOpenCountProvider)}'),
                      child: const Icon(Icons.inbox_outlined),
                    ),
                    constraints: const BoxConstraints(
                      minWidth: Metric.minTarget,
                      minHeight: Metric.minTarget,
                    ),
                    onPressed: () => unawaited(
                      Navigator.of(context).push(
                        MaterialPageRoute<void>(
                          builder: (BuildContext context) => Scaffold(
                            appBar: AppBar(title: Text(l10n.tabInbox)),
                            body: const InboxScreen(),
                          ),
                        ),
                      ),
                    ),
                  ),
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
                          child: hideBar
                              ? inset(
                                  const Column(
                                    children: <Widget>[
                                      ConnectionBanner(),
                                      Expanded(child: ChatScreen()),
                                    ],
                                  ),
                                )
                              : inset(const ChatScreen()),
                        ),
                        if (tab == 2)
                          inset(const VisualizeScreen(), bottom: true)
                        else
                          const SizedBox.shrink(),
                        _TabSlot(
                          active: tab == 3,
                          child: inset(const InboxScreen(), bottom: true),
                        ),
                      ],
                    ),
                  ),
                ),
                if (!hideBar)
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

class _BottomNav extends ConsumerWidget {
  const _BottomNav();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    if (MediaQuery.sizeOf(context).width >= Metric.threePanelBreakpoint) {
      return const SizedBox.shrink();
    }
    return GlassNav(
      selected: ref.watch(activeTabProvider),
      solid: ref.watch(activeTabProvider) == 1,
      inboxCount: ref.watch(inboxOpenCountProvider),
      onSelect: (int i) => ref.read(activeTabProvider.notifier).show(i),
    );
  }
}

class KeyboardLiftController {
  final ValueNotifier<double> settled = ValueNotifier<double>(0);

  void dispose() => settled.dispose();
}

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
    if (MediaQuery.viewInsetsOf(context).bottom == 0 &&
        (below - _below).abs() > 0.5) {
      setState(() => _below = below);
    }
  }

  @override
  Widget build(BuildContext context) {
    final double inset = MediaQuery.viewInsetsOf(context).bottom;
    final double reserved = _below + MediaQuery.paddingOf(context).bottom;
    final double lift = math.max(0, inset - reserved);
    if (inset == 0) {
      WidgetsBinding.instance.addPostFrameCallback((Duration _) => _measure());
    }
    if (lift != _lastLift) {
      final bool closing = lift < _lastLift;
      _lastLift = lift;
      _settle?.cancel();
      if (closing && lift > 0) {
        WidgetsBinding.instance.addPostFrameCallback((Duration _) {
          if (_lastLift > 0 && widget.controller.settled.value != 0) widget.controller.settled.value = 0;
        });
      } else if (lift == 0) {
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
    return TickerMode(
      enabled: widget.active,
      child: MediaQuery(
        data: widget.active ? live : (_frozen ?? live),
        child: widget.child,
      ),
    );
  }
}

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
