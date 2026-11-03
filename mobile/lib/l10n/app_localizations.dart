import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:intl/intl.dart' as intl;

import 'app_localizations_en.dart';
import 'app_localizations_vi.dart';

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
  static const List<Locale> supportedLocales = <Locale>[
    Locale('en'),
    Locale('vi'),
  ];

  /// No description provided for @appName.
  ///
  /// In vi, this message translates to:
  /// **'Agent Console'**
  String get appName;

  /// No description provided for @tabSessions.
  ///
  /// In vi, this message translates to:
  /// **'Session'**
  String get tabSessions;

  /// No description provided for @tabChat.
  ///
  /// In vi, this message translates to:
  /// **'Chat'**
  String get tabChat;

  /// No description provided for @tabVisualize.
  ///
  /// In vi, this message translates to:
  /// **'Visualize'**
  String get tabVisualize;

  /// No description provided for @connecting.
  ///
  /// In vi, this message translates to:
  /// **'Đang kết nối'**
  String get connecting;

  /// No description provided for @connectionLostRetry.
  ///
  /// In vi, this message translates to:
  /// **'Mất kết nối, thử lại sau {seconds}s'**
  String connectionLostRetry(int seconds);

  /// No description provided for @connected.
  ///
  /// In vi, this message translates to:
  /// **'Đã kết nối'**
  String get connected;

  /// No description provided for @retryNow.
  ///
  /// In vi, this message translates to:
  /// **'Thử lại'**
  String get retryNow;

  /// No description provided for @retry.
  ///
  /// In vi, this message translates to:
  /// **'Thử lại'**
  String get retry;

  /// No description provided for @cancel.
  ///
  /// In vi, this message translates to:
  /// **'Huỷ'**
  String get cancel;

  /// No description provided for @close.
  ///
  /// In vi, this message translates to:
  /// **'Đóng'**
  String get close;

  /// No description provided for @stateNeedsInput.
  ///
  /// In vi, this message translates to:
  /// **'Cần input'**
  String get stateNeedsInput;

  /// No description provided for @stateRunning.
  ///
  /// In vi, this message translates to:
  /// **'Đang chạy'**
  String get stateRunning;

  /// No description provided for @stateDone.
  ///
  /// In vi, this message translates to:
  /// **'Xong'**
  String get stateDone;

  /// No description provided for @stateExited.
  ///
  /// In vi, this message translates to:
  /// **'Đã thoát'**
  String get stateExited;

  /// No description provided for @sessionsEmptyTitle.
  ///
  /// In vi, this message translates to:
  /// **'Chưa có session nào'**
  String get sessionsEmptyTitle;

  /// No description provided for @sessionsEmptyBody.
  ///
  /// In vi, this message translates to:
  /// **'Các session Claude Code trên Mac sẽ hiện ở đây.'**
  String get sessionsEmptyBody;

  /// No description provided for @searchHint.
  ///
  /// In vi, this message translates to:
  /// **'Tìm session'**
  String get searchHint;

  /// No description provided for @searchNoMatch.
  ///
  /// In vi, this message translates to:
  /// **'Không có session khớp “{query}”.'**
  String searchNoMatch(String query);

  /// No description provided for @loadFailed.
  ///
  /// In vi, this message translates to:
  /// **'Không thể tải dữ liệu. Vui lòng thử lại.'**
  String get loadFailed;

  /// No description provided for @newSession.
  ///
  /// In vi, this message translates to:
  /// **'Session mới'**
  String get newSession;

  /// No description provided for @newSessionFolder.
  ///
  /// In vi, this message translates to:
  /// **'Thư mục'**
  String get newSessionFolder;

  /// No description provided for @newSessionModel.
  ///
  /// In vi, this message translates to:
  /// **'Model'**
  String get newSessionModel;

  /// No description provided for @modelDefault.
  ///
  /// In vi, this message translates to:
  /// **'Mặc định'**
  String get modelDefault;

  /// No description provided for @newSessionNoProjects.
  ///
  /// In vi, this message translates to:
  /// **'Chưa có thư mục gần đây trên Mac.'**
  String get newSessionNoProjects;

  /// No description provided for @newSessionConfirmTitle.
  ///
  /// In vi, this message translates to:
  /// **'Mở session mới?'**
  String get newSessionConfirmTitle;

  /// No description provided for @newSessionConfirmBody.
  ///
  /// In vi, this message translates to:
  /// **'Claude sẽ chạy trong {folder} trên Mac.'**
  String newSessionConfirmBody(String folder);

  /// No description provided for @newSessionOpen.
  ///
  /// In vi, this message translates to:
  /// **'Mở'**
  String get newSessionOpen;

  /// No description provided for @newSessionStarted.
  ///
  /// In vi, this message translates to:
  /// **'Mac đã mở session mới.'**
  String get newSessionStarted;

  /// No description provided for @newSessionFailed.
  ///
  /// In vi, this message translates to:
  /// **'Không thể mở session mới. Vui lòng thử lại.'**
  String get newSessionFailed;

  /// No description provided for @runningSubagents.
  ///
  /// In vi, this message translates to:
  /// **'{count} subagent đang chạy'**
  String runningSubagents(int count);

  /// No description provided for @subagentsShow.
  ///
  /// In vi, this message translates to:
  /// **'Mở {count} subagent'**
  String subagentsShow(int count);

  /// No description provided for @subagentsHide.
  ///
  /// In vi, this message translates to:
  /// **'Thu gọn {count} subagent'**
  String subagentsHide(int count);

  /// No description provided for @justNow.
  ///
  /// In vi, this message translates to:
  /// **'Vừa xong'**
  String get justNow;

  /// No description provided for @minutesShort.
  ///
  /// In vi, this message translates to:
  /// **'{n} phút'**
  String minutesShort(int n);

  /// No description provided for @hoursShort.
  ///
  /// In vi, this message translates to:
  /// **'{n} giờ'**
  String hoursShort(int n);

  /// No description provided for @daysShort.
  ///
  /// In vi, this message translates to:
  /// **'{n} ngày'**
  String daysShort(int n);

  /// No description provided for @viewOnly.
  ///
  /// In vi, this message translates to:
  /// **'Chỉ xem'**
  String get viewOnly;

  /// No description provided for @viewOnlyReason.
  ///
  /// In vi, this message translates to:
  /// **'Session mở ngoài Console — chỉ xem'**
  String get viewOnlyReason;

  /// No description provided for @controlConsole.
  ///
  /// In vi, this message translates to:
  /// **'Console'**
  String get controlConsole;

  /// No description provided for @controlJob.
  ///
  /// In vi, this message translates to:
  /// **'Job nền'**
  String get controlJob;

  /// No description provided for @chatSelectSession.
  ///
  /// In vi, this message translates to:
  /// **'Chọn một session để xem chat.'**
  String get chatSelectSession;

  /// No description provided for @chatEmpty.
  ///
  /// In vi, this message translates to:
  /// **'Chưa có tin nhắn nào.'**
  String get chatEmpty;

  /// No description provided for @chatLoadOlder.
  ///
  /// In vi, this message translates to:
  /// **'Tải tin cũ hơn'**
  String get chatLoadOlder;

  /// No description provided for @chatNewMessages.
  ///
  /// In vi, this message translates to:
  /// **'Tin mới'**
  String get chatNewMessages;

  /// No description provided for @composerHint.
  ///
  /// In vi, this message translates to:
  /// **'Nhắn cho session này'**
  String get composerHint;

  /// No description provided for @send.
  ///
  /// In vi, this message translates to:
  /// **'Gửi'**
  String get send;

  /// No description provided for @sending.
  ///
  /// In vi, this message translates to:
  /// **'Đang gửi'**
  String get sending;

  /// No description provided for @sendFailed.
  ///
  /// In vi, this message translates to:
  /// **'Không thể gửi tin nhắn. Vui lòng thử lại.'**
  String get sendFailed;

  /// No description provided for @pendingPermission.
  ///
  /// In vi, this message translates to:
  /// **'Cần permission: {tool}'**
  String pendingPermission(String tool);

  /// No description provided for @pendingQuestion.
  ///
  /// In vi, this message translates to:
  /// **'Câu hỏi từ Claude'**
  String get pendingQuestion;

  /// No description provided for @allow.
  ///
  /// In vi, this message translates to:
  /// **'Cho phép'**
  String get allow;

  /// No description provided for @deny.
  ///
  /// In vi, this message translates to:
  /// **'Từ chối'**
  String get deny;

  /// No description provided for @answerOnMac.
  ///
  /// In vi, this message translates to:
  /// **'Trả lời trên Mac'**
  String get answerOnMac;

  /// No description provided for @multiSelectOnMac.
  ///
  /// In vi, this message translates to:
  /// **'Câu hỏi này cho chọn nhiều đáp án. Trả lời trên Mac.'**
  String get multiSelectOnMac;

  /// No description provided for @answerFailed.
  ///
  /// In vi, this message translates to:
  /// **'Không thể gửi câu trả lời. Vui lòng thử lại.'**
  String get answerFailed;

  /// No description provided for @toolsExpand.
  ///
  /// In vi, this message translates to:
  /// **'Mở rộng'**
  String get toolsExpand;

  /// No description provided for @toolsCollapse.
  ///
  /// In vi, this message translates to:
  /// **'Thu gọn'**
  String get toolsCollapse;

  /// No description provided for @pairTitle.
  ///
  /// In vi, this message translates to:
  /// **'Ghép đôi với Mac'**
  String get pairTitle;

  /// No description provided for @pairIntro.
  ///
  /// In vi, this message translates to:
  /// **'Trên Mac, mở Agent Console, bấm Điện thoại rồi Ghép đôi điện thoại.'**
  String get pairIntro;

  /// No description provided for @pairScan.
  ///
  /// In vi, this message translates to:
  /// **'Quét mã QR'**
  String get pairScan;

  /// No description provided for @pairManual.
  ///
  /// In vi, this message translates to:
  /// **'Nhập thủ công'**
  String get pairManual;

  /// No description provided for @pairPasteJson.
  ///
  /// In vi, this message translates to:
  /// **'Dán nội dung mã QR (JSON)'**
  String get pairPasteJson;

  /// No description provided for @pairHost.
  ///
  /// In vi, this message translates to:
  /// **'Địa chỉ Mac (IP hoặc tên .local)'**
  String get pairHost;

  /// No description provided for @pairPort.
  ///
  /// In vi, this message translates to:
  /// **'Cổng'**
  String get pairPort;

  /// No description provided for @pairFingerprint.
  ///
  /// In vi, this message translates to:
  /// **'Vân tay chứng chỉ (SHA-256)'**
  String get pairFingerprint;

  /// No description provided for @pairCode.
  ///
  /// In vi, this message translates to:
  /// **'Mã ghép đôi (8 ký tự)'**
  String get pairCode;

  /// No description provided for @pairDeviceName.
  ///
  /// In vi, this message translates to:
  /// **'Tên thiết bị'**
  String get pairDeviceName;

  /// No description provided for @pairConnect.
  ///
  /// In vi, this message translates to:
  /// **'Ghép đôi'**
  String get pairConnect;

  /// No description provided for @pairWaiting.
  ///
  /// In vi, this message translates to:
  /// **'Chờ Mac cho phép…'**
  String get pairWaiting;

  /// No description provided for @pairFailed.
  ///
  /// In vi, this message translates to:
  /// **'Không thể ghép đôi. Vui lòng thử lại.'**
  String get pairFailed;

  /// No description provided for @pairInvalid.
  ///
  /// In vi, this message translates to:
  /// **'Mã ghép đôi không hợp lệ.'**
  String get pairInvalid;

  /// No description provided for @pairFingerprintMismatch.
  ///
  /// In vi, this message translates to:
  /// **'Chứng chỉ của Mac không khớp vân tay đã ghim. Kết nối bị từ chối.'**
  String get pairFingerprintMismatch;

  /// No description provided for @pairDenied.
  ///
  /// In vi, this message translates to:
  /// **'Mac đã từ chối thiết bị này hoặc mã đã hết hạn.'**
  String get pairDenied;

  /// No description provided for @pairUnreachable.
  ///
  /// In vi, this message translates to:
  /// **'Không kết nối được tới Mac. Kiểm tra mạng và địa chỉ.'**
  String get pairUnreachable;

  /// No description provided for @pairCameraDenied.
  ///
  /// In vi, this message translates to:
  /// **'Chưa có quyền dùng camera. Dùng nhập thủ công.'**
  String get pairCameraDenied;

  /// No description provided for @macsTitle.
  ///
  /// In vi, this message translates to:
  /// **'Mac đã ghép'**
  String get macsTitle;

  /// No description provided for @macsAdd.
  ///
  /// In vi, this message translates to:
  /// **'Thêm Mac'**
  String get macsAdd;

  /// No description provided for @macsActive.
  ///
  /// In vi, this message translates to:
  /// **'Đang dùng'**
  String get macsActive;

  /// No description provided for @macsUse.
  ///
  /// In vi, this message translates to:
  /// **'Dùng Mac này'**
  String get macsUse;

  /// No description provided for @macsRemove.
  ///
  /// In vi, this message translates to:
  /// **'Gỡ Mac'**
  String get macsRemove;

  /// No description provided for @macsRemoveConfirm.
  ///
  /// In vi, this message translates to:
  /// **'Gỡ “{name}” khỏi điện thoại?'**
  String macsRemoveConfirm(String name);

  /// No description provided for @vizSuggested.
  ///
  /// In vi, this message translates to:
  /// **'Gợi ý'**
  String get vizSuggested;

  /// No description provided for @vizPhoneNoPermission.
  ///
  /// In vi, this message translates to:
  /// **'Mac chưa cấp quyền ghi màn hình cho Agent Console.'**
  String get vizPhoneNoPermission;

  /// No description provided for @vizPhoneNoWindow.
  ///
  /// In vi, this message translates to:
  /// **'Mac chưa có cửa sổ máy ảo đang hiện.'**
  String get vizPhoneNoWindow;

  /// No description provided for @vizPhoneStarting.
  ///
  /// In vi, this message translates to:
  /// **'Đang chờ hình từ Mac…'**
  String get vizPhoneStarting;

  /// No description provided for @vizPhoneError.
  ///
  /// In vi, this message translates to:
  /// **'Không thể xem máy ảo: {message}'**
  String vizPhoneError(String message);

  /// No description provided for @vizWebUrl.
  ///
  /// In vi, this message translates to:
  /// **'Địa chỉ trang (localhost trên Mac)'**
  String get vizWebUrl;

  /// No description provided for @vizWebRefresh.
  ///
  /// In vi, this message translates to:
  /// **'Chụp lại'**
  String get vizWebRefresh;

  /// No description provided for @vizWebNoUrl.
  ///
  /// In vi, this message translates to:
  /// **'Session này chưa có địa chỉ web. Nhập địa chỉ ở trên.'**
  String get vizWebNoUrl;

  /// No description provided for @vizWebFailed.
  ///
  /// In vi, this message translates to:
  /// **'Không thể chụp trang. Vui lòng thử lại.'**
  String get vizWebFailed;

  /// No description provided for @vizLoadFailed.
  ///
  /// In vi, this message translates to:
  /// **'Không thể tải visualize. Vui lòng thử lại.'**
  String get vizLoadFailed;

  /// No description provided for @vizSelectSession.
  ///
  /// In vi, this message translates to:
  /// **'Chọn một session để xem visualize.'**
  String get vizSelectSession;

  /// No description provided for @vizBundleMissing.
  ///
  /// In vi, this message translates to:
  /// **'Thiếu bundle viz. Chạy tool/sync_viz.sh rồi build lại.'**
  String get vizBundleMissing;

  /// No description provided for @notifNeedsInputTitle.
  ///
  /// In vi, this message translates to:
  /// **'Session cần input'**
  String get notifNeedsInputTitle;

  /// No description provided for @notifNeedsInputBody.
  ///
  /// In vi, this message translates to:
  /// **'{name} đang chờ bạn.'**
  String notifNeedsInputBody(String name);

  /// No description provided for @pullToRefresh.
  ///
  /// In vi, this message translates to:
  /// **'Kéo để tải lại'**
  String get pullToRefresh;

  /// No description provided for @connectionRevoked.
  ///
  /// In vi, this message translates to:
  /// **'Mac đã thu hồi thiết bị này. Hãy ghép đôi lại.'**
  String get connectionRevoked;

  /// No description provided for @openMacs.
  ///
  /// In vi, this message translates to:
  /// **'Mac đã ghép'**
  String get openMacs;

  /// No description provided for @routeHome.
  ///
  /// In vi, this message translates to:
  /// **'Wi-Fi nhà'**
  String get routeHome;

  /// No description provided for @routeInternet.
  ///
  /// In vi, this message translates to:
  /// **'Internet'**
  String get routeInternet;

  /// No description provided for @connectedVia.
  ///
  /// In vi, this message translates to:
  /// **'Đã kết nối · {route}'**
  String connectedVia(String route);

  /// No description provided for @noRouteToMac.
  ///
  /// In vi, this message translates to:
  /// **'Không tới được Mac. Bật Truy cập qua internet trên Mac hoặc vào cùng Wi-Fi.'**
  String get noRouteToMac;

  /// No description provided for @takeover.
  ///
  /// In vi, this message translates to:
  /// **'Tiếp quản'**
  String get takeover;

  /// No description provided for @takeoverHint.
  ///
  /// In vi, this message translates to:
  /// **'Session mở ngoài Console — chỉ xem. Có thể tiếp quản để nhắn từ điện thoại.'**
  String get takeoverHint;

  /// No description provided for @takeoverTitle.
  ///
  /// In vi, this message translates to:
  /// **'Tiếp quản session này?'**
  String get takeoverTitle;

  /// No description provided for @takeoverBody.
  ///
  /// In vi, this message translates to:
  /// **'Mac sẽ mở lại session này trong Terminal của Console (claude --resume). Session gốc phải đã đóng. Nếu Claude hỏi gì, trả lời trên Mac.'**
  String get takeoverBody;

  /// No description provided for @takeoverRunning.
  ///
  /// In vi, this message translates to:
  /// **'Đang tiếp quản…'**
  String get takeoverRunning;

  /// No description provided for @takeoverFailed.
  ///
  /// In vi, this message translates to:
  /// **'Không tiếp quản được: {reason}'**
  String takeoverFailed(String reason);

  /// No description provided for @micStart.
  ///
  /// In vi, this message translates to:
  /// **'Nói để nhập'**
  String get micStart;

  /// No description provided for @micStop.
  ///
  /// In vi, this message translates to:
  /// **'Dừng và nhận dạng'**
  String get micStop;

  /// No description provided for @micCancel.
  ///
  /// In vi, this message translates to:
  /// **'Huỷ nhận dạng'**
  String get micCancel;

  /// No description provided for @transcribing.
  ///
  /// In vi, this message translates to:
  /// **'Đang nhận dạng…'**
  String get transcribing;

  /// No description provided for @voicePermissionDenied.
  ///
  /// In vi, this message translates to:
  /// **'Chưa cho phép dùng micro.'**
  String get voicePermissionDenied;

  /// No description provided for @voiceOpenSettings.
  ///
  /// In vi, this message translates to:
  /// **'Mở cài đặt'**
  String get voiceOpenSettings;

  /// No description provided for @voiceNoSpeech.
  ///
  /// In vi, this message translates to:
  /// **'Không nghe thấy tiếng nói. Thử nói gần micro hơn.'**
  String get voiceNoSpeech;

  /// No description provided for @voiceTooLong.
  ///
  /// In vi, this message translates to:
  /// **'Đã dừng ở 60 giây.'**
  String get voiceTooLong;

  /// No description provided for @voiceEngineError.
  ///
  /// In vi, this message translates to:
  /// **'Nhận dạng lỗi. Thử lại.'**
  String get voiceEngineError;

  /// No description provided for @voiceRecorderError.
  ///
  /// In vi, this message translates to:
  /// **'Không ghi âm được.'**
  String get voiceRecorderError;

  /// No description provided for @keepAliveTitle.
  ///
  /// In vi, this message translates to:
  /// **'Giữ kết nối nền?'**
  String get keepAliveTitle;

  /// No description provided for @keepAliveBody.
  ///
  /// In vi, this message translates to:
  /// **'Để nhận thông báo khi session cần input hoặc xong, app cần chạy nền. Android sẽ hiện một thông báo nhỏ \"Agent Console đang theo dõi\". Tắt được trong Mac đã ghép.'**
  String get keepAliveBody;

  /// No description provided for @keepAliveEnable.
  ///
  /// In vi, this message translates to:
  /// **'Bật'**
  String get keepAliveEnable;

  /// No description provided for @notNow.
  ///
  /// In vi, this message translates to:
  /// **'Để sau'**
  String get notNow;

  /// No description provided for @keepAliveSetting.
  ///
  /// In vi, this message translates to:
  /// **'Giữ kết nối nền'**
  String get keepAliveSetting;

  /// No description provided for @keepAliveHint.
  ///
  /// In vi, this message translates to:
  /// **'Nhận thông báo khi app ở nền. Hiện một thông báo nhỏ cố định.'**
  String get keepAliveHint;

  /// No description provided for @muteSession.
  ///
  /// In vi, this message translates to:
  /// **'Tắt thông báo session này'**
  String get muteSession;

  /// No description provided for @unmuteSession.
  ///
  /// In vi, this message translates to:
  /// **'Bật lại thông báo session này'**
  String get unmuteSession;

  /// No description provided for @takeoverStopBody.
  ///
  /// In vi, this message translates to:
  /// **'Dừng session trên Mac (Claude desktop / VS Code) và mở lại trong Console để nhắn từ điện thoại?'**
  String get takeoverStopBody;

  /// No description provided for @takeoverStopConfirm.
  ///
  /// In vi, this message translates to:
  /// **'Dừng và tiếp quản'**
  String get takeoverStopConfirm;

  /// No description provided for @takeoverBusyTitle.
  ///
  /// In vi, this message translates to:
  /// **'Session đang chạy'**
  String get takeoverBusyTitle;

  /// No description provided for @takeoverBusyBody.
  ///
  /// In vi, this message translates to:
  /// **'Session đang chạy — lượt hiện tại sẽ bị dừng giữa chừng.'**
  String get takeoverBusyBody;

  /// No description provided for @takeoverBusyConfirm.
  ///
  /// In vi, this message translates to:
  /// **'Vẫn dừng'**
  String get takeoverBusyConfirm;

  /// No description provided for @chatQueued.
  ///
  /// In vi, this message translates to:
  /// **'đang chờ'**
  String get chatQueued;

  /// No description provided for @chatSending.
  ///
  /// In vi, this message translates to:
  /// **'đang gửi'**
  String get chatSending;

  /// No description provided for @chatSendFailed.
  ///
  /// In vi, this message translates to:
  /// **'Chưa gửi được'**
  String get chatSendFailed;

  /// No description provided for @chatUpdating.
  ///
  /// In vi, this message translates to:
  /// **'Đang cập nhật'**
  String get chatUpdating;

  /// No description provided for @pictureFailed.
  ///
  /// In vi, this message translates to:
  /// **'Không tải được ảnh'**
  String get pictureFailed;

  /// No description provided for @pictureClose.
  ///
  /// In vi, this message translates to:
  /// **'Đóng ảnh'**
  String get pictureClose;

  /// No description provided for @pictureOpen.
  ///
  /// In vi, this message translates to:
  /// **'Mở ảnh'**
  String get pictureOpen;
}

class _AppL10nDelegate extends LocalizationsDelegate<AppL10n> {
  const _AppL10nDelegate();

  @override
  Future<AppL10n> load(Locale locale) {
    return SynchronousFuture<AppL10n>(lookupAppL10n(locale));
  }

  @override
  bool isSupported(Locale locale) =>
      <String>['en', 'vi'].contains(locale.languageCode);

  @override
  bool shouldReload(_AppL10nDelegate old) => false;
}

AppL10n lookupAppL10n(Locale locale) {
  // Lookup logic when only language code is specified.
  switch (locale.languageCode) {
    case 'en':
      return AppL10nEn();
    case 'vi':
      return AppL10nVi();
  }

  throw FlutterError(
    'AppL10n.delegate failed to load unsupported locale "$locale". This is likely '
    'an issue with the localizations generation tool. Please file an issue '
    'on GitHub with a reproducible sample app and the gen-l10n configuration '
    'that was used.',
  );
}
