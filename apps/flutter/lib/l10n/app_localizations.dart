import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:intl/intl.dart' as intl;

import 'app_localizations_en.dart';

// ignore_for_file: type=lint

/// Callers can lookup localized strings with an instance of AppL10n
/// returned by `AppL10n.of(context)`.
///
/// Applications need to include `AppL10n.delegate()` in their app's
/// `localizationDelegates` list, and the locales they support in the app's
/// `supportedLocales` list. For example:
///
/// ```dart
/// import 'l10n/app_localizations.dart';
///
/// return MaterialApp(
///   localizationsDelegates: AppL10n.localizationsDelegates,
///   supportedLocales: AppL10n.supportedLocales,
///   home: MyApplicationHome(),
/// );
/// ```
///
/// ## Update pubspec.yaml
///
/// Please make sure to update your pubspec.yaml to include the following
/// packages:
///
/// ```yaml
/// dependencies:
///   # Internationalization support.
///   flutter_localizations:
///     sdk: flutter
///   intl: any # Use the pinned version from flutter_localizations
///
///   # Rest of dependencies
/// ```
///
/// ## iOS Applications
///
/// iOS applications define key application metadata, including supported
/// locales, in an Info.plist file that is built into the application bundle.
/// To configure the locales supported by your app, you’ll need to edit this
/// file.
///
/// First, open your project’s ios/Runner.xcworkspace Xcode workspace file.
/// Then, in the Project Navigator, open the Info.plist file under the Runner
/// project’s Runner folder.
///
/// Next, select the Information Property List item, select Add Item from the
/// Editor menu, then select Localizations from the pop-up menu.
///
/// Select and expand the newly-created Localizations item then, for each
/// locale your application supports, add a new item and select the locale
/// you wish to add from the pop-up menu in the Value field. This list should
/// be consistent with the languages listed in the AppL10n.supportedLocales
/// property.
abstract class AppL10n {
  AppL10n(String locale)
    : localeName = intl.Intl.canonicalizedLocale(locale.toString());

  final String localeName;

  static AppL10n of(BuildContext context) {
    return Localizations.of<AppL10n>(context, AppL10n)!;
  }

  static const LocalizationsDelegate<AppL10n> delegate = _AppL10nDelegate();

  /// A list of this localizations delegate along with the default localizations
  /// delegates.
  ///
  /// Returns a list of localizations delegates containing this delegate along with
  /// GlobalMaterialLocalizations.delegate, GlobalCupertinoLocalizations.delegate,
  /// and GlobalWidgetsLocalizations.delegate.
  ///
  /// Additional delegates can be added by appending to this list in
  /// MaterialApp. This list does not have to be used at all if a custom list
  /// of delegates is preferred or required.
  static const List<LocalizationsDelegate<dynamic>> localizationsDelegates =
      <LocalizationsDelegate<dynamic>>[
        delegate,
        GlobalMaterialLocalizations.delegate,
        GlobalCupertinoLocalizations.delegate,
        GlobalWidgetsLocalizations.delegate,
      ];

  /// A list of this localizations delegate's supported locales.
  static const List<Locale> supportedLocales = <Locale>[Locale('en')];

  /// Shown in the Chat screen and the Paired Macs screen and the Sessions tab.
  ///
  /// In en, this message translates to:
  /// **'Cancel'**
  String get appCancel;

  /// Shown in the Chat screen and pairing with a Mac.
  ///
  /// In en, this message translates to:
  /// **'Close'**
  String get appClose;

  /// Shown in the app shell and the Paired Macs screen.
  ///
  /// In en, this message translates to:
  /// **'Connected'**
  String get appConnected;

  /// Shown in the app shell. $route is Home Wi-Fi or Internet.
  ///
  /// In en, this message translates to:
  /// **'Connected · {route}'**
  String appConnectedVia(String route);

  /// Shown in the app shell and the Paired Macs screen.
  ///
  /// In en, this message translates to:
  /// **'Connecting'**
  String get appConnecting;

  /// Shown in the app shell. $seconds is the wait before the next try, in seconds.
  ///
  /// In en, this message translates to:
  /// **'Connection lost, retrying in {seconds}s'**
  String appConnectionLostRetry(int seconds);

  /// Shown in the app shell.
  ///
  /// In en, this message translates to:
  /// **'The Mac revoked this device. Pair again.'**
  String get appConnectionRevoked;

  /// Shown in the app shell. $count is how many requests are open.
  ///
  /// In en, this message translates to:
  /// **'{count} open'**
  String appInboxOpenCount(int count);

  /// Shown in the app shell.
  ///
  /// In en, this message translates to:
  /// **'To get notified when a session needs input or finishes, the app has to run in the background. Android shows a small \"Agent Console is watching\" notification. You can turn it off under Paired Macs.'**
  String get appKeepAliveBody;

  /// Shown in the app shell.
  ///
  /// In en, this message translates to:
  /// **'Turn on'**
  String get appKeepAliveEnable;

  /// Shown in the app shell.
  ///
  /// In en, this message translates to:
  /// **'Keep the connection in the background?'**
  String get appKeepAliveTitle;

  /// Shown in the Chat screen and the Sessions tab.
  ///
  /// In en, this message translates to:
  /// **'Couldn\'t load data. Please try again.'**
  String get appLoadFailed;

  /// Shown in the app shell.
  ///
  /// In en, this message translates to:
  /// **'Agent Console'**
  String get appName;

  /// Shown in the app shell.
  ///
  /// In en, this message translates to:
  /// **'Can\'t reach the Mac. Turn on Internet access on the Mac or join the same Wi-Fi.'**
  String get appNoRouteToMac;

  /// Shown in the app shell.
  ///
  /// In en, this message translates to:
  /// **'Not now'**
  String get appNotNow;

  /// Shown in the app shell and the Chat screen and Settings.
  ///
  /// In en, this message translates to:
  /// **'Paired Macs'**
  String get appOpenMacs;

  /// Not shown yet.
  ///
  /// In en, this message translates to:
  /// **'Pull to refresh'**
  String get appPullToRefresh;

  /// Shown in the app shell and the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Retry'**
  String get appRetry;

  /// Shown in the app shell.
  ///
  /// In en, this message translates to:
  /// **'Retry now'**
  String get appRetryNow;

  /// Shown in the app shell.
  ///
  /// In en, this message translates to:
  /// **'Home Wi-Fi'**
  String get appRouteHome;

  /// Shown in the app shell.
  ///
  /// In en, this message translates to:
  /// **'Internet'**
  String get appRouteInternet;

  /// Shown in the app shell and Settings.
  ///
  /// In en, this message translates to:
  /// **'Settings'**
  String get appSettingsTitle;

  /// Shown in the app shell.
  ///
  /// In en, this message translates to:
  /// **'Chat'**
  String get appTabChat;

  /// Shown in the app shell.
  ///
  /// In en, this message translates to:
  /// **'Inbox'**
  String get appTabInbox;

  /// Shown in the app shell.
  ///
  /// In en, this message translates to:
  /// **'Sessions'**
  String get appTabSessions;

  /// Shown in the app shell.
  ///
  /// In en, this message translates to:
  /// **'Visualize'**
  String get appTabVisualize;

  /// Shown in the Chat screen and the Sessions tab.
  ///
  /// In en, this message translates to:
  /// **'Take over'**
  String get appTakeover;

  /// Shown in the Chat screen and the Sessions tab. $reason is the reason.
  ///
  /// In en, this message translates to:
  /// **'Could not take over: {reason}'**
  String appTakeoverFailed(String reason);

  /// Shown in the Chat screen and the Sessions tab.
  ///
  /// In en, this message translates to:
  /// **'View only'**
  String get appViewOnly;

  /// Not shown yet.
  ///
  /// In en, this message translates to:
  /// **'Idle'**
  String get chatActivityIdle;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Couldn\'t send the answer. Please try again.'**
  String get chatAnswerFailed;

  /// Not shown yet.
  ///
  /// In en, this message translates to:
  /// **'Answer on the Mac'**
  String get chatAnswerOnMac;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Attach a photo'**
  String get chatAttachPhoto;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Conversation compacted'**
  String get chatCompactionTitle;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Message this session'**
  String get chatComposerHint;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Context'**
  String get chatContext;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Console'**
  String get chatControlConsole;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Background job'**
  String get chatControlJob;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Copied'**
  String get chatCopied;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Copy'**
  String get chatCopyCode;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'No messages yet.'**
  String get chatEmpty;

  /// Not shown yet.
  ///
  /// In en, this message translates to:
  /// **'5h'**
  String get chatLimitFiveHours;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Week'**
  String get chatLimitWeek;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Load older messages'**
  String get chatLoadOlder;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'This question allows several answers. Answer it on the Mac.'**
  String get chatMultiSelectOnMac;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Mute notifications for this session'**
  String get chatMuteSession;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'New messages'**
  String get chatNewMessages;

  /// Not shown yet.
  ///
  /// In en, this message translates to:
  /// **'New messages'**
  String get chatNewMessagesBelow;

  /// Shown in the Chat screen. $tool is the tool Claude wants to use.
  ///
  /// In en, this message translates to:
  /// **'Permission needed: {tool}'**
  String chatPendingPermission(String tool);

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Question from Claude'**
  String get chatPendingQuestion;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Take a photo'**
  String get chatPhotoCamera;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Add a caption (optional)'**
  String get chatPhotoCaptionHint;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Choose from library'**
  String get chatPhotoGallery;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Could not open the camera or library.'**
  String get chatPhotoPickFailed;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Photo sent'**
  String get chatPhotoSent;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Send a photo'**
  String get chatPhotoTitle;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Close picture'**
  String get chatPictureClose;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Could not load the picture'**
  String get chatPictureFailed;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Open picture'**
  String get chatPictureOpen;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'waiting'**
  String get chatQueued;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Select a session to see its chat.'**
  String get chatSelectSession;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Send'**
  String get chatSend;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Sending'**
  String get chatSendBusy;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Couldn\'t send the message. Please try again.'**
  String get chatSendError;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Not sent'**
  String get chatSendFailed;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Send now'**
  String get chatSendNow;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Claude stops the current turn and runs this waiting message right away.'**
  String get chatSendNowBody;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Could not interrupt'**
  String get chatSendNowFailed;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Interrupt the running turn to send now?'**
  String get chatSendNowTitle;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'sending'**
  String get chatSending;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Shell command'**
  String get chatShellHeader;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Send runs it like ! in the terminal · delete the ! to send a message instead'**
  String get chatShellHint;

  /// Shown in the Chat screen. $folder is the folder path.
  ///
  /// In en, this message translates to:
  /// **'runs in {folder} · Claude sees the output'**
  String chatShellRunsIn(String folder);

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Will run'**
  String get chatShellWillRun;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Show output'**
  String get chatShowOutput;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Commands'**
  String get chatSlashHeader;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'The Mac will reopen this session in Console\'s Terminal (claude --resume). The original must be closed. If Claude asks anything, answer on the Mac.'**
  String get chatTakeoverBody;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'The session is running; the current turn will be cut off.'**
  String get chatTakeoverBusyBody;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Session is running'**
  String get chatTakeoverBusyTitle;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Opened outside Console, view only. Take it over to message it from your phone.'**
  String get chatTakeoverHint;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Taking over…'**
  String get chatTakeoverRunning;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Stop the session on the Mac (Claude desktop / VS Code) and reopen it in Console so you can message it from the phone?'**
  String get chatTakeoverStopBody;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Take over this session?'**
  String get chatTakeoverTitle;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'1 tool step'**
  String get chatToolStepOne;

  /// Shown in the Chat screen. $count is the number of tool steps.
  ///
  /// In en, this message translates to:
  /// **'{count} tool steps'**
  String chatToolStepsCount(int count);

  /// Not shown yet.
  ///
  /// In en, this message translates to:
  /// **'Collapse'**
  String get chatToolsCollapse;

  /// Not shown yet.
  ///
  /// In en, this message translates to:
  /// **'Expand'**
  String get chatToolsExpand;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Unmute this session'**
  String get chatUnmuteSession;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Updating'**
  String get chatUpdating;

  /// Not shown yet.
  ///
  /// In en, this message translates to:
  /// **'View only'**
  String get chatViewOnlyChip;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Opened outside Console, view only'**
  String get chatViewOnlyReason;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Allow'**
  String get consentAllow;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Deny'**
  String get consentDeny;

  /// Shown in pairing with a Mac.
  ///
  /// In en, this message translates to:
  /// **'Pair'**
  String get consentPair;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Stop and take over'**
  String get consentStopAndTakeOver;

  /// Shown in the Chat screen.
  ///
  /// In en, this message translates to:
  /// **'Stop anyway'**
  String get consentStopAnyway;

  /// Shown in the Inbox. $action is the choice you made, such as Allow.
  ///
  /// In en, this message translates to:
  /// **'You answered: {action}'**
  String inboxAnsweredHere(String action);

  /// Shown in the Inbox. $action is the choice made on the Mac, such as Allow.
  ///
  /// In en, this message translates to:
  /// **'Answered on the Mac: {action}'**
  String inboxAnsweredOnMac(String action);

  /// Shown in the Inbox.
  ///
  /// In en, this message translates to:
  /// **'The Mac didn\'t take that answer. Try again on the Mac.'**
  String get inboxDeclined;

  /// Shown in the Inbox.
  ///
  /// In en, this message translates to:
  /// **'Done'**
  String get inboxDone;

  /// Shown in the Inbox.
  ///
  /// In en, this message translates to:
  /// **'Requests from your Mac show up here.'**
  String get inboxEmptyBody;

  /// Shown in the Inbox.
  ///
  /// In en, this message translates to:
  /// **'Nothing Needs You'**
  String get inboxEmptyTitle;

  /// Shown in the Inbox.
  ///
  /// In en, this message translates to:
  /// **'Needs You'**
  String get inboxNeedsYou;

  /// Shown in the Inbox.
  ///
  /// In en, this message translates to:
  /// **'Open on Mac'**
  String get inboxOpenOnMac;

  /// Shown in the Inbox.
  ///
  /// In en, this message translates to:
  /// **'Notchling on this Mac has no inbox yet.'**
  String get inboxUnsupported;

  /// The result of an inbox item nobody answered in time. Shown in the Inbox.
  ///
  /// In en, this message translates to:
  /// **'Expired'**
  String get itemExpired;

  /// The result of an inbox item taken back. Shown in the Inbox.
  ///
  /// In en, this message translates to:
  /// **'Withdrawn'**
  String get itemWithdrawn;

  /// The language's own name, as its speakers write it. It is the label of this language in the Language row.
  ///
  /// In en, this message translates to:
  /// **'English'**
  String get languageName;

  /// Shown in the Paired Macs screen.
  ///
  /// In en, this message translates to:
  /// **'In use'**
  String get macsActive;

  /// Shown in the Paired Macs screen.
  ///
  /// In en, this message translates to:
  /// **'Add a Mac'**
  String get macsAdd;

  /// Shown in the Paired Macs screen.
  ///
  /// In en, this message translates to:
  /// **'Not connected'**
  String get macsComputerOffline;

  /// Shown in the Paired Macs screen.
  ///
  /// In en, this message translates to:
  /// **'Revoked'**
  String get macsComputerRevoked;

  /// Shown in the Paired Macs screen.
  ///
  /// In en, this message translates to:
  /// **'Get notifications while the app is in the background. Shows a small permanent notification.'**
  String get macsKeepAliveHint;

  /// Shown in the Paired Macs screen.
  ///
  /// In en, this message translates to:
  /// **'Keep connection in the background'**
  String get macsKeepAliveSetting;

  /// Shown in the Paired Macs screen.
  ///
  /// In en, this message translates to:
  /// **'Remove Mac'**
  String get macsRemove;

  /// Shown in the Paired Macs screen. $name is the Mac's name.
  ///
  /// In en, this message translates to:
  /// **'Remove “{name}” from this phone?'**
  String macsRemoveConfirm(String name);

  /// Shown in the Paired Macs screen.
  ///
  /// In en, this message translates to:
  /// **'Paired Macs'**
  String get macsTitle;

  /// Shown in the Paired Macs screen.
  ///
  /// In en, this message translates to:
  /// **'Use this Mac'**
  String get macsUse;

  /// Not shown yet. $name is the session name.
  ///
  /// In en, this message translates to:
  /// **'{name} is waiting.'**
  String notificationsNeedsInputBody(String name);

  /// Not shown yet.
  ///
  /// In en, this message translates to:
  /// **'Session needs input'**
  String get notificationsNeedsInputTitle;

  /// Shown in pairing with a Mac.
  ///
  /// In en, this message translates to:
  /// **'Camera access is off. Use manual entry.'**
  String get pairingCameraDenied;

  /// Shown in pairing with a Mac.
  ///
  /// In en, this message translates to:
  /// **'Pairing code (8 characters)'**
  String get pairingCode;

  /// Shown in pairing with a Mac.
  ///
  /// In en, this message translates to:
  /// **'The Mac refused this device, or the code expired.'**
  String get pairingDenied;

  /// Not shown yet.
  ///
  /// In en, this message translates to:
  /// **'Device name'**
  String get pairingDeviceName;

  /// Not shown yet.
  ///
  /// In en, this message translates to:
  /// **'Couldn\'t pair. Please try again.'**
  String get pairingFailed;

  /// Shown in pairing with a Mac.
  ///
  /// In en, this message translates to:
  /// **'Certificate fingerprint (SHA-256)'**
  String get pairingFingerprint;

  /// Shown in pairing with a Mac.
  ///
  /// In en, this message translates to:
  /// **'The Mac\'s certificate doesn\'t match the pinned fingerprint. Connection refused.'**
  String get pairingFingerprintMismatch;

  /// Shown in pairing with a Mac.
  ///
  /// In en, this message translates to:
  /// **'Mac address (IP or .local name)'**
  String get pairingHost;

  /// Shown in pairing with a Mac.
  ///
  /// In en, this message translates to:
  /// **'On the Mac, open Agent Console, choose Phone, then Pair a phone.'**
  String get pairingIntro;

  /// Shown in pairing with a Mac.
  ///
  /// In en, this message translates to:
  /// **'The pairing code isn\'t valid.'**
  String get pairingInvalid;

  /// Shown in pairing with a Mac.
  ///
  /// In en, this message translates to:
  /// **'Enter manually'**
  String get pairingManual;

  /// Shown in pairing with a Mac.
  ///
  /// In en, this message translates to:
  /// **'Paste the QR content (JSON)'**
  String get pairingPasteJson;

  /// Shown in pairing with a Mac.
  ///
  /// In en, this message translates to:
  /// **'Port'**
  String get pairingPort;

  /// Shown in pairing with a Mac.
  ///
  /// In en, this message translates to:
  /// **'Scan QR'**
  String get pairingScan;

  /// Shown in pairing with a Mac.
  ///
  /// In en, this message translates to:
  /// **'Pair with a Mac'**
  String get pairingTitle;

  /// Shown in pairing with a Mac.
  ///
  /// In en, this message translates to:
  /// **'Couldn\'t reach the Mac. Check the network and address.'**
  String get pairingUnreachable;

  /// Shown in pairing with a Mac.
  ///
  /// In en, this message translates to:
  /// **'Waiting for the Mac to allow…'**
  String get pairingWaiting;

  /// Shown in the Sessions tab.
  ///
  /// In en, this message translates to:
  /// **'All projects'**
  String get sessionsAllProjects;

  /// Shown in the Sessions tab.
  ///
  /// In en, this message translates to:
  /// **'Archive'**
  String get sessionsArchive;

  /// Shown in the Sessions tab. $name is the session name.
  ///
  /// In en, this message translates to:
  /// **'Archived {name}'**
  String sessionsArchivedToast(String name);

  /// Shown in the Sessions tab.
  ///
  /// In en, this message translates to:
  /// **'No sessions'**
  String get sessionsComputerNoSessions;

  /// Shown in the Sessions tab. $n is days.
  ///
  /// In en, this message translates to:
  /// **'{n} d'**
  String sessionsDaysShort(int n);

  /// Shown in the Sessions tab.
  ///
  /// In en, this message translates to:
  /// **'Claude Code sessions on your Mac show up here.'**
  String get sessionsEmptyBody;

  /// Shown in the Sessions tab.
  ///
  /// In en, this message translates to:
  /// **'No sessions yet'**
  String get sessionsEmptyTitle;

  /// Shown in the Sessions tab.
  ///
  /// In en, this message translates to:
  /// **'No sessions in this project.'**
  String get sessionsFilteredEmpty;

  /// Shown in the Sessions tab. $n is hours.
  ///
  /// In en, this message translates to:
  /// **'{n} h'**
  String sessionsHoursShort(int n);

  /// Shown in the Sessions tab.
  ///
  /// In en, this message translates to:
  /// **'Just now'**
  String get sessionsJustNow;

  /// Shown in the Sessions tab. $n is minutes.
  ///
  /// In en, this message translates to:
  /// **'{n} min'**
  String sessionsMinutesShort(int n);

  /// Shown in the Sessions tab.
  ///
  /// In en, this message translates to:
  /// **'Default'**
  String get sessionsModelDefault;

  /// Shown in the Sessions tab.
  ///
  /// In en, this message translates to:
  /// **'New session'**
  String get sessionsNewSession;

  /// Shown in the Sessions tab. $folder is the folder path.
  ///
  /// In en, this message translates to:
  /// **'Claude will run in {folder} on the Mac.'**
  String sessionsNewSessionConfirmBody(String folder);

  /// Shown in the Sessions tab.
  ///
  /// In en, this message translates to:
  /// **'Start a new session?'**
  String get sessionsNewSessionConfirmTitle;

  /// Shown in the Sessions tab.
  ///
  /// In en, this message translates to:
  /// **'Couldn\'t start a new session. Please try again.'**
  String get sessionsNewSessionFailed;

  /// Shown in the Sessions tab.
  ///
  /// In en, this message translates to:
  /// **'Folder'**
  String get sessionsNewSessionFolder;

  /// Shown in the Sessions tab.
  ///
  /// In en, this message translates to:
  /// **'Model'**
  String get sessionsNewSessionModel;

  /// Shown in the Sessions tab.
  ///
  /// In en, this message translates to:
  /// **'No recent folders on the Mac.'**
  String get sessionsNewSessionNoProjects;

  /// Shown in the Sessions tab.
  ///
  /// In en, this message translates to:
  /// **'Start'**
  String get sessionsNewSessionOpen;

  /// Shown in the Sessions tab.
  ///
  /// In en, this message translates to:
  /// **'The Mac started a new session.'**
  String get sessionsNewSessionStarted;

  /// Shown in the Sessions tab. $count is the number of subagents.
  ///
  /// In en, this message translates to:
  /// **'{count} subagents running'**
  String sessionsRunningSubagents(int count);

  /// Shown in the Sessions tab.
  ///
  /// In en, this message translates to:
  /// **'Search sessions'**
  String get sessionsSearchHint;

  /// Shown in the Sessions tab. $query is what you typed.
  ///
  /// In en, this message translates to:
  /// **'No sessions match “{query}”.'**
  String sessionsSearchNoMatch(String query);

  /// Shown in the Sessions tab.
  ///
  /// In en, this message translates to:
  /// **'Ended'**
  String get sessionsStateExited;

  /// Shown in the Sessions tab.
  ///
  /// In en, this message translates to:
  /// **'Needs Input'**
  String get sessionsStateNeedsInput;

  /// Shown in the Sessions tab. $count is the number of subagents.
  ///
  /// In en, this message translates to:
  /// **'Hide {count} subagents'**
  String sessionsSubagentsHide(int count);

  /// Shown in the Sessions tab. $count is the number of subagents.
  ///
  /// In en, this message translates to:
  /// **'Show {count} subagents'**
  String sessionsSubagentsShow(int count);

  /// Shown in the Sessions tab.
  ///
  /// In en, this message translates to:
  /// **'Take over'**
  String get sessionsTakeoverShort;

  /// Shown in the Sessions tab.
  ///
  /// In en, this message translates to:
  /// **'Undo'**
  String get sessionsUndo;

  /// Shown in Settings.
  ///
  /// In en, this message translates to:
  /// **'Notifications'**
  String get settingsNotifications;

  /// Shown in Settings.
  ///
  /// In en, this message translates to:
  /// **'Notification sounds'**
  String get settingsSound;

  /// Shown in Settings.
  ///
  /// In en, this message translates to:
  /// **'A soft sound for new messages and sessions that need you while the app is open.'**
  String get settingsSoundHint;

  /// Shown in Settings.
  ///
  /// In en, this message translates to:
  /// **'Appearance'**
  String get settingsTheme;

  /// Shown in Settings.
  ///
  /// In en, this message translates to:
  /// **'Dark'**
  String get settingsThemeDark;

  /// Shown in Settings.
  ///
  /// In en, this message translates to:
  /// **'Light'**
  String get settingsThemeLight;

  /// Shown in Settings.
  ///
  /// In en, this message translates to:
  /// **'System'**
  String get settingsThemeSystem;

  /// A session that finished its turn. Shown in the Sessions tab.
  ///
  /// In en, this message translates to:
  /// **'Done'**
  String get stateDone;

  /// A session that is working on a turn. Shown in the Chat screen and the Sessions tab.
  ///
  /// In en, this message translates to:
  /// **'Working'**
  String get stateRunning;

  /// Shown in the Visualize tab.
  ///
  /// In en, this message translates to:
  /// **'The viz bundle is missing. Run tool/sync_viz.sh and rebuild.'**
  String get visualizeBundleMissing;

  /// Shown in the Visualize tab.
  ///
  /// In en, this message translates to:
  /// **'Couldn\'t load the visualization. Please try again.'**
  String get visualizeLoadFailed;

  /// Not shown yet. $message is the reason.
  ///
  /// In en, this message translates to:
  /// **'Couldn\'t show the device: {message}'**
  String visualizePhoneError(String message);

  /// Not shown yet.
  ///
  /// In en, this message translates to:
  /// **'The Mac hasn\'t granted Screen Recording to Agent Console.'**
  String get visualizePhoneNoPermission;

  /// Not shown yet.
  ///
  /// In en, this message translates to:
  /// **'No device window is showing on the Mac.'**
  String get visualizePhoneNoWindow;

  /// Not shown yet.
  ///
  /// In en, this message translates to:
  /// **'Waiting for frames from the Mac…'**
  String get visualizePhoneStarting;

  /// Shown in the Visualize tab.
  ///
  /// In en, this message translates to:
  /// **'Select a session to see its visualization.'**
  String get visualizeSelectSession;

  /// Shown in the Visualize tab.
  ///
  /// In en, this message translates to:
  /// **'Suggested'**
  String get visualizeSuggested;

  /// Not shown yet.
  ///
  /// In en, this message translates to:
  /// **'Running on the Mac'**
  String get visualizeWebCandidates;

  /// Shown in the Visualize tab.
  ///
  /// In en, this message translates to:
  /// **'Couldn\'t capture the page. Please try again.'**
  String get visualizeWebFailed;

  /// Shown in the Visualize tab.
  ///
  /// In en, this message translates to:
  /// **'Start the dev server on the Mac and refresh, or pick another address below.'**
  String get visualizeWebHint;

  /// Shown in the Visualize tab.
  ///
  /// In en, this message translates to:
  /// **'This session has no web address yet. Enter one above.'**
  String get visualizeWebNoUrl;

  /// Shown in the Visualize tab.
  ///
  /// In en, this message translates to:
  /// **'Capture again'**
  String get visualizeWebRefresh;

  /// Shown in the Visualize tab.
  ///
  /// In en, this message translates to:
  /// **'Page address (localhost on the Mac)'**
  String get visualizeWebUrl;

  /// Shown in dictation.
  ///
  /// In en, this message translates to:
  /// **'Transcription failed. Try again.'**
  String get voiceEngineError;

  /// Shown in dictation.
  ///
  /// In en, this message translates to:
  /// **'Cancel transcription'**
  String get voiceMicCancel;

  /// Shown in dictation.
  ///
  /// In en, this message translates to:
  /// **'Dictate'**
  String get voiceMicStart;

  /// Shown in dictation.
  ///
  /// In en, this message translates to:
  /// **'Stop and transcribe'**
  String get voiceMicStop;

  /// Shown in dictation.
  ///
  /// In en, this message translates to:
  /// **'No speech heard. Try speaking closer to the mic.'**
  String get voiceNoSpeech;

  /// Shown in dictation.
  ///
  /// In en, this message translates to:
  /// **'Open settings'**
  String get voiceOpenSettings;

  /// Shown in dictation.
  ///
  /// In en, this message translates to:
  /// **'Microphone access is off.'**
  String get voicePermissionDenied;

  /// Shown in dictation.
  ///
  /// In en, this message translates to:
  /// **'Could not record.'**
  String get voiceRecorderError;

  /// Shown in dictation.
  ///
  /// In en, this message translates to:
  /// **'Stopped at 60 seconds.'**
  String get voiceTooLong;

  /// Shown in dictation.
  ///
  /// In en, this message translates to:
  /// **'Transcribing…'**
  String get voiceTranscribing;
}

class _AppL10nDelegate extends LocalizationsDelegate<AppL10n> {
  const _AppL10nDelegate();

  @override
  Future<AppL10n> load(Locale locale) {
    return SynchronousFuture<AppL10n>(lookupAppL10n(locale));
  }

  @override
  bool isSupported(Locale locale) =>
      <String>['en'].contains(locale.languageCode);

  @override
  bool shouldReload(_AppL10nDelegate old) => false;
}

AppL10n lookupAppL10n(Locale locale) {
  // Lookup logic when only language code is specified.
  switch (locale.languageCode) {
    case 'en':
      return AppL10nEn();
  }

  throw FlutterError(
    'AppL10n.delegate failed to load unsupported locale "$locale". This is likely '
    'an issue with the localizations generation tool. Please file an issue '
    'on GitHub with a reproducible sample app and the gen-l10n configuration '
    'that was used.',
  );
}
