import 'package:flutter_riverpod/flutter_riverpod.dart';

final NotifierProvider<ActiveTabNotifier, int> activeTabProvider = NotifierProvider<ActiveTabNotifier, int>(ActiveTabNotifier.new);

class ActiveTabNotifier extends Notifier<int> {
  @override
  int build() => 0;

  void show(int index) => state = index;
}
