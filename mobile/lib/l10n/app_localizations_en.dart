// ignore: unused_import
import 'package:intl/intl.dart' as intl;
import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for English (`en`).
class AppL10nEn extends AppL10n {
  AppL10nEn([String locale = 'en']) : super(locale);

  @override
  String get appName => 'Agent Console';

  @override
  String get tabSessions => 'Sessions';

  @override
  String get tabChat => 'Chat';

  @override
  String get tabVisualize => 'Visualize';

  @override
  String get connecting => 'Connecting';

  @override
  String connectionLostRetry(int seconds) {
    return 'Connection lost, retrying in ${seconds}s';
  }

  @override
  String get connected => 'Connected';

  @override
  String get retryNow => 'Retry now';

  @override
  String get retry => 'Retry';

  @override
  String get cancel => 'Cancel';

  @override
  String get close => 'Close';

  @override
  String get stateNeedsInput => 'Needs Input';

  @override
  String get stateRunning => 'Working';

  @override
  String get stateDone => 'Done';

  @override
  String get stateExited => 'Ended';

  @override
  String get sessionsEmptyTitle => 'No sessions yet';

  @override
  String get sessionsEmptyBody =>
      'Claude Code sessions on your Mac show up here.';

  @override
  String get searchHint => 'Search sessions';

  @override
  String searchNoMatch(String query) {
    return 'No sessions match “$query”.';
  }

  @override
  String get loadFailed => 'Couldn\'t load data. Please try again.';

  @override
  String get newSession => 'New session';

  @override
  String get newSessionFolder => 'Folder';

  @override
  String get newSessionModel => 'Model';

  @override
  String get modelDefault => 'Default';

  @override
  String get newSessionNoProjects => 'No recent folders on the Mac.';

  @override
  String get newSessionConfirmTitle => 'Start a new session?';

  @override
  String newSessionConfirmBody(String folder) {
    return 'Claude will run in $folder on the Mac.';
  }

  @override
  String get newSessionOpen => 'Start';

  @override
  String get newSessionStarted => 'The Mac started a new session.';

  @override
  String get newSessionFailed =>
      'Couldn\'t start a new session. Please try again.';

  @override
  String runningSubagents(int count) {
    return '$count subagents running';
  }

  @override
  String subagentsShow(int count) {
    return 'Show $count subagents';
  }

  @override
  String subagentsHide(int count) {
    return 'Hide $count subagents';
  }

  @override
  String get justNow => 'Just now';

  @override
  String minutesShort(int n) {
    return '$n min';
  }

  @override
  String hoursShort(int n) {
    return '$n h';
  }

  @override
  String daysShort(int n) {
    return '$n d';
  }

  @override
  String get viewOnly => 'View only';

  @override
  String get viewOnlyReason => 'Opened outside Console, view only';

  @override
  String get controlConsole => 'Console';

  @override
  String get controlJob => 'Background job';

  @override
  String get chatSelectSession => 'Select a session to see its chat.';

  @override
  String get chatEmpty => 'No messages yet.';

  @override
  String get chatLoadOlder => 'Load older messages';

  @override
  String get chatNewMessages => 'New messages';

  @override
  String get composerHint => 'Message this session';

  @override
  String get send => 'Send';

  @override
  String get sending => 'Sending';

  @override
  String get sendFailed => 'Couldn\'t send the message. Please try again.';

  @override
  String pendingPermission(String tool) {
    return 'Permission needed: $tool';
  }

  @override
  String get pendingQuestion => 'Question from Claude';

  @override
  String get allow => 'Allow';

  @override
  String get deny => 'Deny';

  @override
  String get answerOnMac => 'Answer on the Mac';

  @override
  String get multiSelectOnMac =>
      'This question allows several answers. Answer it on the Mac.';

  @override
  String get answerFailed => 'Couldn\'t send the answer. Please try again.';

  @override
  String get toolsExpand => 'Expand';

  @override
  String get toolsCollapse => 'Collapse';

  @override
  String get pairTitle => 'Pair with a Mac';

  @override
  String get pairIntro =>
      'On the Mac, open Agent Console, choose Phone, then Pair a phone.';

  @override
  String get pairScan => 'Scan QR';

  @override
  String get pairManual => 'Enter manually';

  @override
  String get pairPasteJson => 'Paste the QR content (JSON)';

  @override
  String get pairHost => 'Mac address (IP or .local name)';

  @override
  String get pairPort => 'Port';

  @override
  String get pairFingerprint => 'Certificate fingerprint (SHA-256)';

  @override
  String get pairCode => 'Pairing code (8 characters)';

  @override
  String get pairDeviceName => 'Device name';

  @override
  String get pairConnect => 'Pair';

  @override
  String get pairWaiting => 'Waiting for the Mac to allow…';

  @override
  String get pairFailed => 'Couldn\'t pair. Please try again.';

  @override
  String get pairInvalid => 'The pairing code isn\'t valid.';

  @override
  String get pairFingerprintMismatch =>
      'The Mac\'s certificate doesn\'t match the pinned fingerprint. Connection refused.';

  @override
  String get pairDenied => 'The Mac refused this device, or the code expired.';

  @override
  String get pairUnreachable =>
      'Couldn\'t reach the Mac. Check the network and address.';

  @override
  String get pairCameraDenied => 'Camera access is off. Use manual entry.';

  @override
  String get macsTitle => 'Paired Macs';

  @override
  String get macsAdd => 'Add a Mac';

  @override
  String get macsActive => 'In use';

  @override
  String get macsUse => 'Use this Mac';

  @override
  String get macsRemove => 'Remove Mac';

  @override
  String macsRemoveConfirm(String name) {
    return 'Remove “$name” from this phone?';
  }

  @override
  String get vizSuggested => 'Suggested';

  @override
  String get vizPhoneNoPermission =>
      'The Mac hasn\'t granted Screen Recording to Agent Console.';

  @override
  String get vizPhoneNoWindow => 'No device window is showing on the Mac.';

  @override
  String get vizPhoneStarting => 'Waiting for frames from the Mac…';

  @override
  String vizPhoneError(String message) {
    return 'Couldn\'t show the device: $message';
  }

  @override
  String get vizWebUrl => 'Page address (localhost on the Mac)';

  @override
  String get vizWebRefresh => 'Capture again';

  @override
  String get vizWebNoUrl =>
      'This session has no web address yet. Enter one above.';

  @override
  String get vizWebHint =>
      'Start the dev server on the Mac and refresh, or pick another address below.';

  @override
  String get vizWebCandidates => 'Running on the Mac';

  @override
  String get vizWebFailed => 'Couldn\'t capture the page. Please try again.';

  @override
  String get vizLoadFailed =>
      'Couldn\'t load the visualization. Please try again.';

  @override
  String get vizSelectSession => 'Select a session to see its visualization.';

  @override
  String get vizBundleMissing =>
      'The viz bundle is missing. Run tool/sync_viz.sh and rebuild.';

  @override
  String get notifNeedsInputTitle => 'Session needs input';

  @override
  String notifNeedsInputBody(String name) {
    return '$name is waiting.';
  }

  @override
  String get pullToRefresh => 'Pull to refresh';

  @override
  String get connectionRevoked => 'The Mac revoked this device. Pair again.';

  @override
  String get openMacs => 'Paired Macs';

  @override
  String get routeHome => 'Home Wi-Fi';

  @override
  String get routeInternet => 'Internet';

  @override
  String connectedVia(String route) {
    return 'Connected · $route';
  }

  @override
  String get noRouteToMac =>
      'Can\'t reach the Mac. Turn on Internet access on the Mac or join the same Wi-Fi.';

  @override
  String get takeover => 'Take over';

  @override
  String get takeoverHint =>
      'Opened outside Console, view only. Take it over to message it from your phone.';

  @override
  String get takeoverTitle => 'Take over this session?';

  @override
  String get takeoverBody =>
      'The Mac will reopen this session in Console\'s Terminal (claude --resume). The original must be closed. If Claude asks anything, answer on the Mac.';

  @override
  String get takeoverRunning => 'Taking over…';

  @override
  String takeoverFailed(String reason) {
    return 'Could not take over: $reason';
  }

  @override
  String get micStart => 'Dictate';

  @override
  String get micStop => 'Stop and transcribe';

  @override
  String get micCancel => 'Cancel transcription';

  @override
  String get transcribing => 'Transcribing…';

  @override
  String get voicePermissionDenied => 'Microphone access is off.';

  @override
  String get voiceOpenSettings => 'Open settings';

  @override
  String get voiceNoSpeech =>
      'No speech heard. Try speaking closer to the mic.';

  @override
  String get voiceTooLong => 'Stopped at 60 seconds.';

  @override
  String get voiceEngineError => 'Transcription failed. Try again.';

  @override
  String get voiceRecorderError => 'Could not record.';

  @override
  String get keepAliveTitle => 'Keep the connection in the background?';

  @override
  String get keepAliveBody =>
      'To get notified when a session needs input or finishes, the app has to run in the background. Android shows a small \"Agent Console is watching\" notification. You can turn it off under Paired Macs.';

  @override
  String get keepAliveEnable => 'Turn on';

  @override
  String get notNow => 'Not now';

  @override
  String get keepAliveSetting => 'Keep connection in the background';

  @override
  String get keepAliveHint =>
      'Get notifications while the app is in the background. Shows a small permanent notification.';

  @override
  String get muteSession => 'Mute notifications for this session';

  @override
  String get unmuteSession => 'Unmute this session';

  @override
  String get takeoverStopBody =>
      'Stop the session on the Mac (Claude desktop / VS Code) and reopen it in Console so you can message it from the phone?';

  @override
  String get takeoverStopConfirm => 'Stop and take over';

  @override
  String get takeoverBusyTitle => 'Session is running';

  @override
  String get takeoverBusyBody =>
      'The session is running; the current turn will be cut off.';

  @override
  String get takeoverBusyConfirm => 'Stop anyway';

  @override
  String get chatQueued => 'waiting';

  @override
  String get chatSendNow => 'Send now';

  @override
  String get chatSendNowTitle => 'Interrupt the running turn to send now?';

  @override
  String get chatSendNowBody =>
      'Claude stops the current turn and runs this waiting message right away.';

  @override
  String get chatSendNowFailed => 'Could not interrupt';

  @override
  String get chatSending => 'sending';

  @override
  String get chatSendFailed => 'Not sent';

  @override
  String get chatUpdating => 'Updating';

  @override
  String get pictureFailed => 'Could not load the picture';

  @override
  String get pictureClose => 'Close picture';

  @override
  String get pictureOpen => 'Open picture';

  @override
  String get audioPlay => 'Play audio';

  @override
  String get audioPause => 'Pause audio';

  @override
  String get audioFailed => 'Could not load the audio. Tap to retry';

  @override
  String get allProjects => 'All projects';

  @override
  String get archive => 'Archive';

  @override
  String archivedToast(String name) {
    return 'Archived $name';
  }

  @override
  String get undo => 'Undo';

  @override
  String get sessionsFilteredEmpty => 'No sessions in this project.';

  @override
  String get chatContext => 'Context';

  @override
  String get limit5h => '5h';

  @override
  String get limitWeek => 'Week';

  @override
  String get shellHeader => 'Shell command';

  @override
  String shellRunsIn(String folder) {
    return 'runs in $folder · Claude sees the output';
  }

  @override
  String get shellHint =>
      'Send runs it like ! in the terminal · delete the ! to send a message instead';

  @override
  String get shellWillRun => 'Will run';

  @override
  String get slashHeader => 'Commands';

  @override
  String toolStepsCount(int count) {
    return '$count tool steps';
  }

  @override
  String get toolStepOne => '1 tool step';

  @override
  String get compactionTitle => 'Conversation compacted';

  @override
  String get showOutput => 'Show output';

  @override
  String get copyCode => 'Copy';

  @override
  String get copied => 'Copied';

  @override
  String get takeoverShort => 'Take over';

  @override
  String get activityIdle => 'Idle';

  @override
  String get viewOnlyChip => 'View only';

  @override
  String get newMessagesBelow => 'New messages';

  @override
  String get attachPhoto => 'Attach a photo';

  @override
  String get photoCamera => 'Take a photo';

  @override
  String get photoGallery => 'Choose from library';

  @override
  String get photoPickFailed => 'Could not open the camera or library.';

  @override
  String get photoSent => 'Photo sent';

  @override
  String get photoTitle => 'Send a photo';

  @override
  String get photoCaptionHint => 'Add a caption (optional)';

  @override
  String get settingsTitle => 'Settings';

  @override
  String get settingsTheme => 'Appearance';

  @override
  String get themeSystem => 'System';

  @override
  String get themeDark => 'Dark';

  @override
  String get themeLight => 'Light';

  @override
  String get settingsNotifications => 'Notifications';

  @override
  String get settingsSound => 'Notification sounds';

  @override
  String get settingsSoundHint =>
      'A soft sound for new messages and sessions that need you while the app is open.';
}
