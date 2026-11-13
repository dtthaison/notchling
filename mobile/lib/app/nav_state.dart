import 'package:flutter/widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// The root Navigator. The alert banner sits above it (so a pushed chat cannot cover it) and opens chats through it.
final GlobalKey<NavigatorState> appNavigatorKey = GlobalKey<NavigatorState>();

/// Which bottom tab is showing on a phone: 0 Session, 1 Visualize. The chat is not a tab: it is pushed over the list.
final NotifierProvider<ActiveTabNotifier, int> activeTabProvider = NotifierProvider<ActiveTabNotifier, int>(ActiveTabNotifier.new);

class ActiveTabNotifier extends Notifier<int> {
  @override
  int build() => 0;

  void show(int index) => state = index;
}

/// True while the pushed chat screen is open on a phone. Read by openChat (no second push) and by the alerts (no banner
/// for the chat the user is looking at).
final NotifierProvider<ChatRouteOpenNotifier, bool> chatRouteOpenProvider = NotifierProvider<ChatRouteOpenNotifier, bool>(ChatRouteOpenNotifier.new);

class ChatRouteOpenNotifier extends Notifier<bool> {
  @override
  bool build() => false;

  // The chat route closes while the app (and its container) may already be going away.
  void set({required bool open}) {
    if (ref.mounted) state = open;
  }
}
