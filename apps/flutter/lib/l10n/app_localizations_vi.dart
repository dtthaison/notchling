// ignore: unused_import
import 'package:intl/intl.dart' as intl;
import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for Vietnamese (`vi`).
class AppL10nVi extends AppL10n {
  AppL10nVi([String locale = 'vi']) : super(locale);

  @override
  String get appName => 'Agent Console';

  @override
  String get tabSessions => 'Session';

  @override
  String get tabChat => 'Chat';

  @override
  String get tabVisualize => 'Visualize';

  @override
  String get tabInbox => 'Hộp thư';

  @override
  String get inboxNeedsYou => 'Cần bạn';

  @override
  String get inboxDone => 'Xong';

  @override
  String get inboxEmptyTitle => 'Không có gì cần bạn';

  @override
  String get inboxEmptyBody => 'Yêu cầu từ Mac sẽ hiện ở đây.';

  @override
  String get inboxUnsupported => 'Notchling trên Mac này chưa có hộp thư.';

  @override
  String get inboxOpenOnMac => 'Mở trên Mac';

  @override
  String inboxAnsweredOnMac(String action) {
    return 'Đã trả lời trên Mac: $action';
  }

  @override
  String inboxAnsweredHere(String action) {
    return 'Bạn đã trả lời: $action';
  }

  @override
  String get inboxDeclined =>
      'Mac không nhận câu trả lời này. Thử lại trên Mac.';

  @override
  String get inboxExpired => 'Đã hết hạn';

  @override
  String get inboxWithdrawn => 'Đã rút lại';

  @override
  String inboxOpenCount(int count) {
    return '$count chưa xử lý';
  }

  @override
  String get connecting => 'Đang kết nối';

  @override
  String connectionLostRetry(int seconds) {
    return 'Mất kết nối, thử lại sau ${seconds}s';
  }

  @override
  String get connected => 'Đã kết nối';

  @override
  String get retryNow => 'Thử lại';

  @override
  String get retry => 'Thử lại';

  @override
  String get cancel => 'Huỷ';

  @override
  String get close => 'Đóng';

  @override
  String get stateNeedsInput => 'Cần bạn';

  @override
  String get stateRunning => 'Đang chạy';

  @override
  String get stateDone => 'Xong';

  @override
  String get stateExited => 'Đã thoát';

  @override
  String get sessionsEmptyTitle => 'Chưa có session nào';

  @override
  String get sessionsEmptyBody =>
      'Các session Claude Code trên Mac sẽ hiện ở đây.';

  @override
  String get searchHint => 'Tìm session';

  @override
  String searchNoMatch(String query) {
    return 'Không có session khớp “$query”.';
  }

  @override
  String get loadFailed => 'Không thể tải dữ liệu. Vui lòng thử lại.';

  @override
  String get newSession => 'Session mới';

  @override
  String get newSessionFolder => 'Thư mục';

  @override
  String get newSessionModel => 'Model';

  @override
  String get modelDefault => 'Mặc định';

  @override
  String get newSessionNoProjects => 'Chưa có thư mục gần đây trên Mac.';

  @override
  String get newSessionConfirmTitle => 'Mở session mới?';

  @override
  String newSessionConfirmBody(String folder) {
    return 'Claude sẽ chạy trong $folder trên Mac.';
  }

  @override
  String get newSessionOpen => 'Mở';

  @override
  String get newSessionStarted => 'Mac đã mở session mới.';

  @override
  String get newSessionFailed => 'Không thể mở session mới. Vui lòng thử lại.';

  @override
  String runningSubagents(int count) {
    return '$count subagent đang chạy';
  }

  @override
  String subagentsShow(int count) {
    return 'Mở $count subagent';
  }

  @override
  String subagentsHide(int count) {
    return 'Thu gọn $count subagent';
  }

  @override
  String get justNow => 'Vừa xong';

  @override
  String minutesShort(int n) {
    return '$n phút';
  }

  @override
  String hoursShort(int n) {
    return '$n giờ';
  }

  @override
  String daysShort(int n) {
    return '$n ngày';
  }

  @override
  String get viewOnly => 'Chỉ xem';

  @override
  String get viewOnlyReason => 'Session mở ngoài Console — chỉ xem';

  @override
  String get controlConsole => 'Console';

  @override
  String get controlJob => 'Job nền';

  @override
  String get chatSelectSession => 'Chọn một session để xem chat.';

  @override
  String get chatEmpty => 'Chưa có tin nhắn nào.';

  @override
  String get chatLoadOlder => 'Tải tin cũ hơn';

  @override
  String get chatNewMessages => 'Tin mới';

  @override
  String get composerHint => 'Nhắn cho session này';

  @override
  String get send => 'Gửi';

  @override
  String get sending => 'Đang gửi';

  @override
  String get sendFailed => 'Không thể gửi tin nhắn. Vui lòng thử lại.';

  @override
  String pendingPermission(String tool) {
    return 'Cần cấp quyền: $tool';
  }

  @override
  String get pendingQuestion => 'Câu hỏi từ Claude';

  @override
  String get allow => 'Cho phép';

  @override
  String get deny => 'Từ chối';

  @override
  String get answerOnMac => 'Trả lời trên Mac';

  @override
  String get multiSelectOnMac =>
      'Câu hỏi này cho chọn nhiều đáp án. Trả lời trên Mac.';

  @override
  String get answerFailed => 'Không thể gửi câu trả lời. Vui lòng thử lại.';

  @override
  String get toolsExpand => 'Mở rộng';

  @override
  String get toolsCollapse => 'Thu gọn';

  @override
  String get pairTitle => 'Ghép đôi với Mac';

  @override
  String get pairIntro =>
      'Trên Mac, mở Agent Console, bấm Điện thoại rồi Ghép đôi điện thoại.';

  @override
  String get pairScan => 'Quét mã QR';

  @override
  String get pairManual => 'Nhập thủ công';

  @override
  String get pairPasteJson => 'Dán nội dung mã QR (JSON)';

  @override
  String get pairHost => 'Địa chỉ Mac (IP hoặc tên .local)';

  @override
  String get pairPort => 'Cổng';

  @override
  String get pairFingerprint => 'Vân tay chứng chỉ (SHA-256)';

  @override
  String get pairCode => 'Mã ghép đôi (8 ký tự)';

  @override
  String get pairDeviceName => 'Tên thiết bị';

  @override
  String get pairConnect => 'Ghép đôi';

  @override
  String get pairWaiting => 'Chờ Mac cho phép…';

  @override
  String get pairFailed => 'Không thể ghép đôi. Vui lòng thử lại.';

  @override
  String get pairInvalid => 'Mã ghép đôi không hợp lệ.';

  @override
  String get pairFingerprintMismatch =>
      'Chứng chỉ của Mac không khớp vân tay đã ghim. Kết nối bị từ chối.';

  @override
  String get pairDenied => 'Mac đã từ chối thiết bị này hoặc mã đã hết hạn.';

  @override
  String get pairUnreachable =>
      'Không kết nối được tới Mac. Kiểm tra mạng và địa chỉ.';

  @override
  String get pairCameraDenied =>
      'Chưa có quyền dùng camera. Dùng nhập thủ công.';

  @override
  String get macsTitle => 'Mac đã ghép';

  @override
  String get macsAdd => 'Thêm Mac';

  @override
  String get macsActive => 'Đang dùng';

  @override
  String get macsUse => 'Dùng Mac này';

  @override
  String get macsRemove => 'Gỡ Mac';

  @override
  String get computerOffline => 'Chưa kết nối';

  @override
  String get computerRevoked => 'Đã thu hồi';

  @override
  String get computerNoSessions => 'Không có session';

  @override
  String macsRemoveConfirm(String name) {
    return 'Gỡ “$name” khỏi điện thoại?';
  }

  @override
  String get vizSuggested => 'Gợi ý';

  @override
  String get vizPhoneNoPermission =>
      'Mac chưa cấp quyền ghi màn hình cho Agent Console.';

  @override
  String get vizPhoneNoWindow => 'Mac chưa có cửa sổ máy ảo đang hiện.';

  @override
  String get vizPhoneStarting => 'Đang chờ hình từ Mac…';

  @override
  String vizPhoneError(String message) {
    return 'Không thể xem máy ảo: $message';
  }

  @override
  String get vizWebUrl => 'Địa chỉ trang (localhost trên Mac)';

  @override
  String get vizWebRefresh => 'Chụp lại';

  @override
  String get vizWebNoUrl =>
      'Session này chưa có địa chỉ web. Nhập địa chỉ ở trên.';

  @override
  String get vizWebHint =>
      'Chạy dev server trên Mac rồi bấm làm mới, hoặc chọn địa chỉ khác bên dưới.';

  @override
  String get vizWebCandidates => 'Đang chạy trên Mac';

  @override
  String get vizWebFailed => 'Không thể chụp trang. Vui lòng thử lại.';

  @override
  String get vizLoadFailed => 'Không thể tải visualize. Vui lòng thử lại.';

  @override
  String get vizSelectSession => 'Chọn một session để xem visualize.';

  @override
  String get vizBundleMissing =>
      'Thiếu bundle viz. Chạy tool/sync_viz.sh rồi build lại.';

  @override
  String get notifNeedsInputTitle => 'Session cần bạn';

  @override
  String notifNeedsInputBody(String name) {
    return '$name đang chờ bạn.';
  }

  @override
  String get pullToRefresh => 'Kéo để tải lại';

  @override
  String get connectionRevoked =>
      'Mac đã thu hồi thiết bị này. Hãy ghép đôi lại.';

  @override
  String get openMacs => 'Mac đã ghép';

  @override
  String get routeHome => 'Wi-Fi nhà';

  @override
  String get routeInternet => 'Internet';

  @override
  String connectedVia(String route) {
    return 'Đã kết nối · $route';
  }

  @override
  String get noRouteToMac =>
      'Không tới được Mac. Bật Truy cập qua internet trên Mac hoặc vào cùng Wi-Fi.';

  @override
  String get takeover => 'Tiếp quản';

  @override
  String get takeoverHint =>
      'Session mở ngoài Console — chỉ xem. Có thể tiếp quản để nhắn từ điện thoại.';

  @override
  String get takeoverTitle => 'Tiếp quản session này?';

  @override
  String get takeoverBody =>
      'Mac sẽ mở lại session này trong Terminal của Console (claude --resume). Session gốc phải đã đóng. Nếu Claude hỏi gì, trả lời trên Mac.';

  @override
  String get takeoverRunning => 'Đang tiếp quản…';

  @override
  String takeoverFailed(String reason) {
    return 'Không tiếp quản được: $reason';
  }

  @override
  String get micStart => 'Nói để nhập';

  @override
  String get micStop => 'Dừng và nhận dạng';

  @override
  String get micCancel => 'Huỷ nhận dạng';

  @override
  String get transcribing => 'Đang nhận dạng…';

  @override
  String get voicePermissionDenied => 'Chưa cho phép dùng micro.';

  @override
  String get voiceOpenSettings => 'Mở cài đặt';

  @override
  String get voiceNoSpeech =>
      'Không nghe thấy tiếng nói. Thử nói gần micro hơn.';

  @override
  String get voiceTooLong => 'Đã dừng ở 60 giây.';

  @override
  String get voiceEngineError => 'Nhận dạng lỗi. Thử lại.';

  @override
  String get voiceRecorderError => 'Không ghi âm được.';

  @override
  String get keepAliveTitle => 'Giữ kết nối nền?';

  @override
  String get keepAliveBody =>
      'Để nhận thông báo khi session cần bạn hoặc xong, app cần chạy nền. Android sẽ hiện một thông báo nhỏ \"Đang theo dõi\". Tắt được trong Mac đã ghép.';

  @override
  String get keepAliveEnable => 'Bật';

  @override
  String get notNow => 'Để sau';

  @override
  String get keepAliveSetting => 'Giữ kết nối nền';

  @override
  String get keepAliveHint =>
      'Nhận thông báo khi app ở nền. Hiện một thông báo nhỏ cố định.';

  @override
  String get muteSession => 'Tắt thông báo session này';

  @override
  String get unmuteSession => 'Bật lại thông báo session này';

  @override
  String get takeoverStopBody =>
      'Dừng session trên Mac (Claude desktop / VS Code) và mở lại trong Console để nhắn từ điện thoại?';

  @override
  String get takeoverStopConfirm => 'Dừng và tiếp quản';

  @override
  String get takeoverBusyTitle => 'Session đang chạy';

  @override
  String get takeoverBusyBody =>
      'Session đang chạy — lượt hiện tại sẽ bị dừng giữa chừng.';

  @override
  String get takeoverBusyConfirm => 'Vẫn dừng';

  @override
  String get chatQueued => 'đang chờ';

  @override
  String get chatSendNow => 'Gửi ngay';

  @override
  String get chatSendNowTitle => 'Ngắt lượt đang chạy để gửi ngay?';

  @override
  String get chatSendNowBody =>
      'Claude sẽ dừng lượt hiện tại và chạy tin đang chờ này ngay.';

  @override
  String get chatSendNowFailed => 'Chưa ngắt được';

  @override
  String get chatSending => 'đang gửi';

  @override
  String get chatSendFailed => 'Chưa gửi được';

  @override
  String get chatUpdating => 'Đang cập nhật';

  @override
  String get pictureFailed => 'Không tải được ảnh';

  @override
  String get pictureClose => 'Đóng ảnh';

  @override
  String get pictureOpen => 'Mở ảnh';

  @override
  String get allProjects => 'Tất cả project';

  @override
  String get archive => 'Lưu trữ';

  @override
  String archivedToast(String name) {
    return 'Đã lưu trữ $name';
  }

  @override
  String get undo => 'Hoàn tác';

  @override
  String get sessionsFilteredEmpty => 'Không có session trong project này.';

  @override
  String get chatContext => 'Ngữ cảnh';

  @override
  String get limit5h => '5 giờ';

  @override
  String get limitWeek => 'Tuần';

  @override
  String get shellHeader => 'Lệnh shell';

  @override
  String shellRunsIn(String folder) {
    return 'chạy trong $folder · Claude thấy kết quả';
  }

  @override
  String get shellHint =>
      'Gửi sẽ chạy như ! trong terminal · xoá ! để gửi tin nhắn thường';

  @override
  String get shellWillRun => 'Sẽ chạy';

  @override
  String get slashHeader => 'Lệnh';

  @override
  String toolStepsCount(int count) {
    return '$count bước công cụ';
  }

  @override
  String get toolStepOne => '1 bước công cụ';

  @override
  String get compactionTitle => 'Đã nén cuộc trò chuyện';

  @override
  String get showOutput => 'Xem kết quả';

  @override
  String get copyCode => 'Chép';

  @override
  String get copied => 'Đã chép';

  @override
  String get takeoverShort => 'Tiếp quản';

  @override
  String get activityIdle => 'Không có hoạt động';

  @override
  String get viewOnlyChip => 'Chỉ xem';

  @override
  String get newMessagesBelow => 'Tin mới';

  @override
  String get attachPhoto => 'Đính kèm ảnh';

  @override
  String get photoCamera => 'Chụp ảnh';

  @override
  String get photoGallery => 'Chọn từ thư viện';

  @override
  String get photoPickFailed => 'Không mở được camera hoặc thư viện.';

  @override
  String get photoSent => 'Đã gửi ảnh';

  @override
  String get photoTitle => 'Gửi ảnh';

  @override
  String get photoCaptionHint => 'Thêm chú thích (tuỳ chọn)';

  @override
  String get settingsTitle => 'Cài đặt';

  @override
  String get settingsTheme => 'Giao diện';

  @override
  String get themeSystem => 'Hệ thống';

  @override
  String get themeDark => 'Tối';

  @override
  String get themeLight => 'Sáng';

  @override
  String get settingsNotifications => 'Thông báo';

  @override
  String get settingsSound => 'Âm thanh thông báo';

  @override
  String get settingsSoundHint =>
      'Phát tiếng nhẹ khi có tin mới hoặc session cần bạn trong lúc đang mở app.';
}
