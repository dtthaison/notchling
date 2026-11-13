import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/nav_state.dart';
import '../../app/shell.dart' show ConnectionBanner;
import '../../app/theme/tokens.dart';
import '../sessions/sessions_notifier.dart';
import 'chat_screen.dart';

/// Opens [sessionId]'s chat. On a phone the chat is pushed over the list (back or the iOS swipe returns to it); from
/// the three-panel width it is already on screen, so only the selection changes. With a chat already on top it is
/// reselected, never pushed twice.
void openChat(BuildContext context, WidgetRef ref, String sessionId) {
  ref.read(selectedSessionIdProvider.notifier).select(sessionId);
  if (MediaQuery.sizeOf(context).width >= Metric.threePanelBreakpoint) return;
  if (ref.read(chatRouteOpenProvider)) return;
  ref.read(chatRouteOpenProvider.notifier).set(open: true);
  unawaited(
    Navigator.of(context).push(
      MaterialPageRoute<void>(builder: (BuildContext context) => const ChatRoutePage()),
    ),
  );
}

/// The pushed chat: the connection banner, then the chat with its own header (which has the back button).
class ChatRoutePage extends ConsumerStatefulWidget {
  const ChatRoutePage({super.key});

  @override
  ConsumerState<ChatRoutePage> createState() => _ChatRoutePageState();
}

class _ChatRoutePageState extends ConsumerState<ChatRoutePage> {
  late final ChatRouteOpenNotifier _open;

  @override
  void initState() {
    super.initState();
    _open = ref.read(chatRouteOpenProvider.notifier);
  }

  @override
  void dispose() {
    // Not during the frame that removes this page: a provider cannot change while the tree is being torn down.
    Future<void>.microtask(() => _open.set(open: false));
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return const Scaffold(
      // The chat lifts its composer with the keyboard itself (KeyboardLift); the page must not resize per frame.
      resizeToAvoidBottomInset: false,
      body: SafeArea(
        bottom: false,
        child: Column(
          children: <Widget>[
            ConnectionBanner(),
            Expanded(child: ChatScreen()),
          ],
        ),
      ),
    );
  }
}
