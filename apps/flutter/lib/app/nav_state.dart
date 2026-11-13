import 'package:flutter_riverpod/flutter_riverpod.dart';

/// Which bottom tab is showing on a phone: 0 Session, 1 Chat, 2 Visualize, 3 Inbox.
final NotifierProvider<ActiveTabNotifier, int> activeTabProvider = NotifierProvider<ActiveTabNotifier, int>(ActiveTabNotifier.new);

class ActiveTabNotifier extends Notifier<int> {
  @override
  int build() => 0;

  void show(int index) => state = index;
}
