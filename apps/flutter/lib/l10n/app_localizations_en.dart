// ignore: unused_import
import 'package:intl/intl.dart' as intl;
import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for English (`en`).
class AppL10nEn extends AppL10n {
  AppL10nEn([String locale = 'en']) : super(locale);

  @override
  String get appCancel => 'Cancel';

  @override
  String get appClose => 'Close';

  @override
  String get appConnected => 'Connected';

  @override
  String appConnectedVia(String route) {
    return 'Connected · $route';
  }

  @override
  String get appConnecting => 'Connecting';

  @override
  String appConnectionLostRetry(int seconds) {
    final intl.NumberFormat secondsNumberFormat =
        intl.NumberFormat.decimalPattern(localeName);
    final String secondsString = secondsNumberFormat.format(seconds);

    return 'Connection lost, retrying in ${secondsString}s';
  }

  @override
  String get appConnectionRevoked => 'The Mac revoked this device. Pair again.';

  @override
  String appInboxOpenCount(int count) {
    final intl.NumberFormat countNumberFormat =
        intl.NumberFormat.decimalPattern(localeName);
    final String countString = countNumberFormat.format(count);

    return '$countString open';
  }

  @override
  String get appKeepAliveBody =>
      'To get notified when a session needs input or finishes, the app has to run in the background. Android shows a small \"Agent Console is watching\" notification. You can turn it off under Paired Macs.';

  @override
  String get appKeepAliveEnable => 'Turn on';

  @override
  String get appKeepAliveTitle => 'Keep the connection in the background?';

  @override
  String get appLoadFailed => 'Couldn\'t load data. Please try again.';

  @override
  String get appName => 'Agent Console';

  @override
  String get appNoRouteToMac =>
      'Can\'t reach the Mac. Turn on Internet access on the Mac or join the same Wi-Fi.';

  @override
  String get appNotNow => 'Not now';

  @override
  String get appOpenMacs => 'Paired Macs';

  @override
  String get appPullToRefresh => 'Pull to refresh';

  @override
  String get appRetry => 'Retry';

  @override
  String get appRetryNow => 'Retry now';

  @override
  String get appRouteHome => 'Home Wi-Fi';

  @override
  String get appRouteInternet => 'Internet';

  @override
  String get appSettingsTitle => 'Settings';

  @override
  String get appTabChat => 'Chat';

  @override
  String get appTabInbox => 'Inbox';

  @override
  String get appTabSessions => 'Sessions';

  @override
  String get appTabVisualize => 'Visualize';

  @override
  String get appTakeover => 'Take over';

  @override
  String appTakeoverFailed(String reason) {
    return 'Could not take over: $reason';
  }

  @override
  String get appViewOnly => 'View only';

  @override
  String get bridgeErrorBusy => 'The Mac is busy. Try again.';

  @override
  String get bridgeErrorDisconnected => 'Not connected to the Mac.';

  @override
  String get bridgeErrorInternal => 'Something went wrong on the Mac.';

  @override
  String get bridgeErrorInvalid => 'The Mac didn\'t accept that request.';

  @override
  String get bridgeErrorNotFound => 'The Mac couldn\'t find that.';

  @override
  String get bridgeErrorNotReachable => 'The Mac couldn\'t reach it.';

  @override
  String get bridgeErrorTimeout => 'The Mac didn\'t answer in time.';

  @override
  String get bridgeErrorUnauthorized =>
      'The Mac doesn\'t accept this phone. Pair again.';

  @override
  String get bridgeErrorUnknown => 'The Mac refused the request.';

  @override
  String get bridgeErrorViewOnly => 'This session is view only.';

  @override
  String bridgeErrorWithDetail(String error, String detail) {
    return '$error $detail';
  }

  @override
  String get chatActivityIdle => 'Idle';

  @override
  String get chatAnswerFailed => 'Couldn\'t send the answer. Please try again.';

  @override
  String get chatAnswerOnMac => 'Answer on the Mac';

  @override
  String get chatAttachPhoto => 'Attach a photo';

  @override
  String get chatCompactionTitle => 'Conversation compacted';

  @override
  String get chatComposerHint => 'Message this session';

  @override
  String get chatContext => 'Context';

  @override
  String get chatControlConsole => 'Console';

  @override
  String get chatControlJob => 'Background job';

  @override
  String get chatCopied => 'Copied';

  @override
  String get chatCopyCode => 'Copy';

  @override
  String get chatEmpty => 'No messages yet.';

  @override
  String get chatLimitFiveHours => '5h';

  @override
  String get chatLimitWeek => 'Week';

  @override
  String get chatLoadOlder => 'Load older messages';

  @override
  String get chatMultiSelectOnMac =>
      'This question allows several answers. Answer it on the Mac.';

  @override
  String get chatMuteSession => 'Mute notifications for this session';

  @override
  String get chatNewMessages => 'New messages';

  @override
  String get chatNewMessagesBelow => 'New messages';

  @override
  String chatPendingPermission(String tool) {
    return 'Permission needed: $tool';
  }

  @override
  String get chatPendingQuestion => 'Question from Claude';

  @override
  String get chatPhotoCamera => 'Take a photo';

  @override
  String get chatPhotoCaptionHint => 'Add a caption (optional)';

  @override
  String get chatPhotoGallery => 'Choose from library';

  @override
  String get chatPhotoPickFailed => 'Could not open the camera or library.';

  @override
  String get chatPhotoSent => 'Photo sent';

  @override
  String get chatPhotoTitle => 'Send a photo';

  @override
  String get chatPictureClose => 'Close picture';

  @override
  String get chatPictureFailed => 'Could not load the picture';

  @override
  String get chatPictureOpen => 'Open picture';

  @override
  String get chatQueued => 'waiting';

  @override
  String get chatSelectSession => 'Select a session to see its chat.';

  @override
  String get chatSend => 'Send';

  @override
  String get chatSendBusy => 'Sending';

  @override
  String get chatSendError => 'Couldn\'t send the message. Please try again.';

  @override
  String get chatSendFailed => 'Not sent';

  @override
  String get chatSendNow => 'Send now';

  @override
  String get chatSendNowBody =>
      'Claude stops the current turn and runs this waiting message right away.';

  @override
  String get chatSendNowFailed => 'Could not interrupt';

  @override
  String get chatSendNowTitle => 'Interrupt the running turn to send now?';

  @override
  String get chatSending => 'sending';

  @override
  String get chatShellHeader => 'Shell command';

  @override
  String get chatShellHint =>
      'Send runs it like ! in the terminal · delete the ! to send a message instead';

  @override
  String chatShellRunsIn(String folder) {
    return 'runs in $folder · Claude sees the output';
  }

  @override
  String get chatShellWillRun => 'Will run';

  @override
  String get chatShowOutput => 'Show output';

  @override
  String get chatSlashHeader => 'Commands';

  @override
  String get chatTakeoverBody =>
      'The Mac will reopen this session in Console\'s Terminal (claude --resume). The original must be closed. If Claude asks anything, answer on the Mac.';

  @override
  String get chatTakeoverBusyBody =>
      'The session is running; the current turn will be cut off.';

  @override
  String get chatTakeoverBusyTitle => 'Session is running';

  @override
  String get chatTakeoverHint =>
      'Opened outside Console, view only. Take it over to message it from your phone.';

  @override
  String get chatTakeoverRunning => 'Taking over…';

  @override
  String get chatTakeoverStopBody =>
      'Stop the session on the Mac (Claude desktop / VS Code) and reopen it in Console so you can message it from the phone?';

  @override
  String get chatTakeoverTitle => 'Take over this session?';

  @override
  String get chatToolStepOne => '1 tool step';

  @override
  String chatToolStepsCount(int count) {
    final intl.NumberFormat countNumberFormat =
        intl.NumberFormat.decimalPattern(localeName);
    final String countString = countNumberFormat.format(count);

    return '$countString tool steps';
  }

  @override
  String get chatToolsCollapse => 'Collapse';

  @override
  String get chatToolsExpand => 'Expand';

  @override
  String get chatUnmuteSession => 'Unmute this session';

  @override
  String get chatUpdating => 'Updating';

  @override
  String get chatViewOnlyChip => 'View only';

  @override
  String get chatViewOnlyReason => 'Opened outside Console, view only';

  @override
  String get consentAllow => 'Allow';

  @override
  String get consentDeny => 'Deny';

  @override
  String get consentPair => 'Pair';

  @override
  String get consentStopAndTakeOver => 'Stop and take over';

  @override
  String get consentStopAnyway => 'Stop anyway';

  @override
  String inboxAnsweredHere(String action) {
    return 'You answered: $action';
  }

  @override
  String inboxAnsweredOnMac(String action) {
    return 'Answered on the Mac: $action';
  }

  @override
  String get inboxDeclined =>
      'The Mac didn\'t take that answer. Try again on the Mac.';

  @override
  String get inboxDone => 'Done';

  @override
  String get inboxEmptyBody => 'Requests from your Mac show up here.';

  @override
  String get inboxEmptyTitle => 'Nothing Needs You';

  @override
  String get inboxNeedsYou => 'Needs You';

  @override
  String get inboxOpenOnMac => 'Open on Mac';

  @override
  String get inboxUnsupported => 'Notchling on this Mac has no inbox yet.';

  @override
  String get itemExpired => 'Expired';

  @override
  String get itemWithdrawn => 'Withdrawn';

  @override
  String get languageName => 'English';

  @override
  String get macsActive => 'In use';

  @override
  String get macsAdd => 'Add a Mac';

  @override
  String get macsComputerOffline => 'Not connected';

  @override
  String get macsComputerRevoked => 'Revoked';

  @override
  String get macsKeepAliveHint =>
      'Get notifications while the app is in the background. Shows a small permanent notification.';

  @override
  String get macsKeepAliveSetting => 'Keep connection in the background';

  @override
  String get macsRemove => 'Remove Mac';

  @override
  String macsRemoveConfirm(String name) {
    return 'Remove “$name” from this phone?';
  }

  @override
  String get macsTitle => 'Paired Macs';

  @override
  String get macsUse => 'Use this Mac';

  @override
  String get notificationsChannelFinished => 'Finished sessions';

  @override
  String get notificationsChannelMessages => 'New messages from Claude';

  @override
  String get notificationsChannelNeedsYou => 'Sessions that need you';

  @override
  String get notificationsFinishedBody => 'Done';

  @override
  String get notificationsNeedsBody => 'Needs your answer';

  @override
  String notificationsNeedsInputBody(String name) {
    return '$name is waiting.';
  }

  @override
  String get notificationsNeedsInputTitle => 'Session needs input';

  @override
  String get notificationsSummaryBody => 'Something new';

  @override
  String get notificationsWatching => 'Watching';

  @override
  String notificationsWatchingSessions(int count) {
    final intl.NumberFormat countNumberFormat =
        intl.NumberFormat.decimalPattern(localeName);
    final String countString = countNumberFormat.format(count);

    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Watching · $countString sessions',
      one: 'Watching · $countString session',
    );
    return '$_temp0';
  }

  @override
  String get pairingCameraDenied => 'Camera access is off. Use manual entry.';

  @override
  String get pairingCode => 'Pairing code (8 characters)';

  @override
  String get pairingDenied =>
      'The Mac refused this device, or the code expired.';

  @override
  String get pairingDeviceName => 'Device name';

  @override
  String get pairingFailed => 'Couldn\'t pair. Please try again.';

  @override
  String get pairingFingerprint => 'Certificate fingerprint (SHA-256)';

  @override
  String get pairingFingerprintMismatch =>
      'The Mac\'s certificate doesn\'t match the pinned fingerprint. Connection refused.';

  @override
  String get pairingHost => 'Mac address (IP or .local name)';

  @override
  String get pairingIntro =>
      'On the Mac, open Agent Console, choose Phone, then Pair a phone.';

  @override
  String get pairingInvalid => 'The pairing code isn\'t valid.';

  @override
  String get pairingManual => 'Enter manually';

  @override
  String get pairingPasteJson => 'Paste the QR content (JSON)';

  @override
  String get pairingPort => 'Port';

  @override
  String get pairingScan => 'Scan QR';

  @override
  String get pairingTitle => 'Pair with a Mac';

  @override
  String get pairingUnreachable =>
      'Couldn\'t reach the Mac. Check the network and address.';

  @override
  String get pairingWaiting => 'Waiting for the Mac to allow…';

  @override
  String get sessionsAllProjects => 'All projects';

  @override
  String get sessionsArchive => 'Archive';

  @override
  String sessionsArchivedToast(String name) {
    return 'Archived $name';
  }

  @override
  String get sessionsComputerNoSessions => 'No sessions';

  @override
  String sessionsDaysShort(int n) {
    final intl.NumberFormat nNumberFormat = intl.NumberFormat.decimalPattern(
      localeName,
    );
    final String nString = nNumberFormat.format(n);

    return '$nString d';
  }

  @override
  String get sessionsEmptyBody =>
      'Claude Code sessions on your Mac show up here.';

  @override
  String get sessionsEmptyTitle => 'No sessions yet';

  @override
  String get sessionsFilteredEmpty => 'No sessions in this project.';

  @override
  String sessionsHoursShort(int n) {
    final intl.NumberFormat nNumberFormat = intl.NumberFormat.decimalPattern(
      localeName,
    );
    final String nString = nNumberFormat.format(n);

    return '$nString h';
  }

  @override
  String get sessionsJustNow => 'Just now';

  @override
  String sessionsMinutesShort(int n) {
    final intl.NumberFormat nNumberFormat = intl.NumberFormat.decimalPattern(
      localeName,
    );
    final String nString = nNumberFormat.format(n);

    return '$nString min';
  }

  @override
  String get sessionsModelDefault => 'Default';

  @override
  String get sessionsNewSession => 'New session';

  @override
  String sessionsNewSessionConfirmBody(String folder) {
    return 'Claude will run in $folder on the Mac.';
  }

  @override
  String get sessionsNewSessionConfirmTitle => 'Start a new session?';

  @override
  String get sessionsNewSessionFailed =>
      'Couldn\'t start a new session. Please try again.';

  @override
  String get sessionsNewSessionFolder => 'Folder';

  @override
  String get sessionsNewSessionModel => 'Model';

  @override
  String get sessionsNewSessionNoProjects => 'No recent folders on the Mac.';

  @override
  String get sessionsNewSessionOpen => 'Start';

  @override
  String get sessionsNewSessionStarted => 'The Mac started a new session.';

  @override
  String sessionsRunningSubagents(int count) {
    final intl.NumberFormat countNumberFormat =
        intl.NumberFormat.decimalPattern(localeName);
    final String countString = countNumberFormat.format(count);

    return '$countString subagents running';
  }

  @override
  String get sessionsSearchHint => 'Search sessions';

  @override
  String sessionsSearchNoMatch(String query) {
    return 'No sessions match “$query”.';
  }

  @override
  String get sessionsStateExited => 'Ended';

  @override
  String get sessionsStateNeedsInput => 'Needs Input';

  @override
  String sessionsSubagentsHide(int count) {
    final intl.NumberFormat countNumberFormat =
        intl.NumberFormat.decimalPattern(localeName);
    final String countString = countNumberFormat.format(count);

    return 'Hide $countString subagents';
  }

  @override
  String sessionsSubagentsShow(int count) {
    final intl.NumberFormat countNumberFormat =
        intl.NumberFormat.decimalPattern(localeName);
    final String countString = countNumberFormat.format(count);

    return 'Show $countString subagents';
  }

  @override
  String get sessionsTakeoverShort => 'Take over';

  @override
  String get sessionsUndo => 'Undo';

  @override
  String get settingsNotifications => 'Notifications';

  @override
  String get settingsSound => 'Notification sounds';

  @override
  String get settingsSoundHint =>
      'A soft sound for new messages and sessions that need you while the app is open.';

  @override
  String get settingsTheme => 'Appearance';

  @override
  String get settingsThemeDark => 'Dark';

  @override
  String get settingsThemeLight => 'Light';

  @override
  String get settingsThemeSystem => 'System';

  @override
  String get stateDone => 'Done';

  @override
  String get stateRunning => 'Working';

  @override
  String get visualizeBundleMissing =>
      'The viz bundle is missing. Run tool/sync_viz.sh and rebuild.';

  @override
  String get visualizeLoadFailed =>
      'Couldn\'t load the visualization. Please try again.';

  @override
  String visualizePhoneError(String message) {
    return 'Couldn\'t show the device: $message';
  }

  @override
  String get visualizePhoneNoPermission =>
      'The Mac hasn\'t granted Screen Recording to Agent Console.';

  @override
  String get visualizePhoneNoWindow =>
      'No device window is showing on the Mac.';

  @override
  String get visualizePhoneStarting => 'Waiting for frames from the Mac…';

  @override
  String get visualizeSelectSession =>
      'Select a session to see its visualization.';

  @override
  String get visualizeSuggested => 'Suggested';

  @override
  String get visualizeWebCandidates => 'Running on the Mac';

  @override
  String get visualizeWebFailed =>
      'Couldn\'t capture the page. Please try again.';

  @override
  String get visualizeWebHint =>
      'Start the dev server on the Mac and refresh, or pick another address below.';

  @override
  String get visualizeWebNoUrl =>
      'This session has no web address yet. Enter one above.';

  @override
  String get visualizeWebRefresh => 'Capture again';

  @override
  String get visualizeWebUrl => 'Page address (localhost on the Mac)';

  @override
  String get voiceEngineError => 'Transcription failed. Try again.';

  @override
  String get voiceMicCancel => 'Cancel transcription';

  @override
  String get voiceMicStart => 'Dictate';

  @override
  String get voiceMicStop => 'Stop and transcribe';

  @override
  String get voiceNoSpeech =>
      'No speech heard. Try speaking closer to the mic.';

  @override
  String get voiceOpenSettings => 'Open settings';

  @override
  String get voicePermissionDenied => 'Microphone access is off.';

  @override
  String get voiceRecorderError => 'Could not record.';

  @override
  String get voiceTooLong => 'Stopped at 60 seconds.';

  @override
  String get voiceTranscribing => 'Transcribing…';
}
