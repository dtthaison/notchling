import 'package:agent_console/core/protocol/models.dart';

final DateTime previewNow = DateTime(2026, 11, 4, 15, 30);

DateTime _ago(Duration d) => previewNow.subtract(d);

Session previewSession({
  required String id,
  required String name,
  required String project,
  required SessionState state,
  SessionControl control = SessionControl.console,
  Duration ago = const Duration(minutes: 2),
  String? activity,
  List<Subagent> subagents = const <Subagent>[],
  PendingPrompt? pending,
  TakeoverInfo? takeover,
  SessionFacts? facts,
}) =>
    Session(
      id: id,
      name: name,
      project: project,
      cwd: '/Users/triet/code/$project',
      entrypoint: 'cli',
      state: state,
      control: control,
      lastActivity: _ago(ago),
      subagents: subagents,
      pending: pending,
      takeover: takeover,
      activity: activity,
      facts: facts,
    );

const SessionFacts previewFacts = SessionFacts(
  contextTokens: 148800,
  contextWindow: 400000,
  model: 'claude-opus-5-5',
  limits: <LimitMeter>[LimitMeter(label: '5h', percent: 62), LimitMeter(label: 'Week', percent: 34)],
);

const SessionFacts previewFactsHigh = SessionFacts(
  contextTokens: 331200,
  contextWindow: 400000,
  limits: <LimitMeter>[LimitMeter(label: '5h', percent: 86), LimitMeter(label: 'Week', percent: 41)],
);

PendingPrompt previewPermission(String sessionId) => PendingPrompt(
      requestId: 'req-1',
      sessionId: sessionId,
      isPermission: true,
      toolName: 'Bash',
      summary: 'git push origin feat/login-fix',
      questions: const <PromptQuestion>[],
    );

List<Session> previewSessions() => <Session>[
      previewSession(
        id: 's1',
        name: 'Sửa lỗi redirect loop khi đăng nhập',
        project: 'web-app',
        state: SessionState.needsInput,
        ago: const Duration(minutes: 1),
        pending: previewPermission('s1'),
        facts: previewFacts,
      ),
      previewSession(
        id: 's2',
        name: 'Refactor payment module',
        project: 'api',
        state: SessionState.running,
        ago: const Duration(seconds: 20),
        activity: 'Editing PaymentService.swift',
        facts: previewFactsHigh,
        subagents: <Subagent>[
          Subagent(id: 'a1', type: 'Explore', description: 'Find every caller of charge()', model: 'haiku', finished: false, running: true, lastActivity: _ago(const Duration(seconds: 8))),
          Subagent(id: 'a2', type: 'general-purpose', description: 'Write unit tests for refunds', model: 'sonnet', finished: false, running: true, lastActivity: _ago(const Duration(seconds: 30))),
          Subagent(id: 'a3', type: 'code-review', description: 'Review the diff', model: 'opus', finished: true, running: false, lastActivity: _ago(const Duration(minutes: 6))),
        ],
      ),
      previewSession(id: 's3', name: 'Update README screenshots', project: 'notchling', state: SessionState.done, ago: const Duration(minutes: 12)),
      previewSession(
        id: 's4',
        name: 'nháp',
        project: 'scratch',
        state: SessionState.done,
        control: SessionControl.viewOnly,
        ago: const Duration(hours: 4),
        takeover: const TakeoverInfo(available: true),
      ),
      previewSession(id: 's5', name: 'Spike: websocket reconnect', project: 'api', state: SessionState.exited, control: SessionControl.viewOnly, ago: const Duration(days: 2)),
    ];

ChatMessage _msg(String id, ChatRole role, String text, {Duration ago = const Duration(minutes: 10), ChatKind kind = ChatKind.text, List<ToolStep> steps = const <ToolStep>[], String? output, String? toolSummary}) =>
    ChatMessage(id: id, role: role, text: text, time: _ago(ago), kind: kind, steps: steps, output: output, toolSummary: toolSummary);

List<ChatMessage> previewConversation() => <ChatMessage>[
      _msg('m1', ChatRole.user, 'Sửa lỗi redirect loop khi đăng nhập, rồi chạy test giúp mình.', ago: const Duration(minutes: 40)),
      _msg('m2', ChatRole.assistant, '''I'll start with the router, then the guard that decides where to send you after sign-in.

## What I found

- `AuthGuard` redirects to **/login** when `session == null`
- the login page pushes **/home**, which runs the guard again before the session is stored
- so the two keep sending you back and forth

Checking the guard now.''', ago: const Duration(minutes: 38)),
      _msg('m3', ChatRole.tools, '', ago: const Duration(minutes: 37), steps: const <ToolStep>[
        ToolStep(name: 'Read', summary: 'lib/auth/auth_guard.dart', output: 'class AuthGuard { ... }'),
        ToolStep(name: 'Grep', summary: 'redirect( in lib/', output: 'lib/auth/auth_guard.dart:42\nlib/auth/login_page.dart:77'),
        ToolStep(name: 'Read', summary: 'lib/auth/login_page.dart'),
        ToolStep(name: 'Glob', summary: 'test/auth/**/*.dart', output: 'test/auth/auth_guard_test.dart'),
      ]),
      _msg('m3b', ChatRole.assistant, 'Found it. The guard runs before the session is stored; making it wait.', ago: const Duration(minutes: 36)),
      _msg('m4', ChatRole.tools, '', ago: const Duration(minutes: 35), steps: const <ToolStep>[
        ToolStep(
          name: 'Edit',
          summary: 'lib/auth/auth_guard.dart',
          path: 'lib/auth/auth_guard.dart',
          diff: <DiffLine>[
            DiffLine(DiffKind.context, '  Future<String?> redirect(BuildContext context) async {'),
            DiffLine(DiffKind.removed, '    if (session == null) return \'/login\';'),
            DiffLine(DiffKind.added, '    // The login page stores the session before it navigates, so wait for it.'),
            DiffLine(DiffKind.added, '    await sessionReady;'),
            DiffLine(DiffKind.added, '    if (session == null) return \'/login\';'),
            DiffLine(DiffKind.context, '    return null;'),
            DiffLine(DiffKind.context, '  }'),
          ],
        ),
      ]),
      _msg('m5', ChatRole.assistant, '''The guard now waits for the stored session. The fix is one line plus a comment:

```dart
Future<String?> redirect(BuildContext context) async {
  await sessionReady; // stored by the login page
  if (session == null) return '/login';
  return null;
}
```

Running the auth tests next.''', ago: const Duration(minutes: 34)),
      _msg('m6', ChatRole.tools, '', ago: const Duration(minutes: 33), steps: const <ToolStep>[
        ToolStep(name: 'Bash', summary: 'flutter test test/auth', output: '00:03 +14: All tests passed!'),
      ]),
      _msg('m7', ChatRole.assistant, 'Conversation summary: fixed the redirect loop in `AuthGuard`; the auth tests pass.', ago: const Duration(minutes: 30), kind: ChatKind.compaction),
      _msg('m8', ChatRole.user, '!git status --short', ago: const Duration(minutes: 5), kind: ChatKind.shell, output: ' M lib/auth/auth_guard.dart\n M test/auth/auth_guard_test.dart\n?? docs/login-flow.md'),
      _msg('m9', ChatRole.assistant, 'Two files changed and one new doc. Want me to push the branch? I need your OK for `git push`.', ago: const Duration(minutes: 1)),
    ];

List<Session> longSessions() => <Session>[
      previewSession(
        id: 'l1',
        name: 'Sửa lỗi redirect loop khi đăng nhập trên web-app',
        project: 'truongbinhtriet',
        state: SessionState.needsInput,
        ago: const Duration(minutes: 1),
        pending: const PendingPrompt(
          requestId: 'rl1',
          sessionId: 'l1',
          isPermission: true,
          toolName: 'Bash',
          summary: 'git push origin feat/redirect-loop-fix --force-with-lease && gh pr create --fill',
          questions: <PromptQuestion>[],
        ),
        facts: previewFacts,
      ),
      previewSession(
        id: 'l2',
        name: 'Notching repo startup',
        project: 'truongbinhtriet',
        state: SessionState.running,
        ago: const Duration(seconds: 12),
        activity: 'Editing Sources/Notchling/Phone/LiveBridgeBackend.swift',
        facts: const SessionFacts(contextTokens: 232500, contextWindow: 400000, limits: <LimitMeter>[LimitMeter(label: '5h', percent: 43), LimitMeter(label: 'Week', percent: 13)]),
        subagents: <Subagent>[
          Subagent(id: 'a1', type: 'general-purpose', description: 'Khảo sát toàn bộ các nơi gọi hàm gửi tin nhắn và ghi lại thứ tự', model: 'claude-sonnet-5-5', finished: false, running: true, lastActivity: _ago(const Duration(seconds: 8))),
          Subagent(id: 'a2', type: 'code-review', description: 'Review diff', model: 'opus', finished: true, running: false, lastActivity: _ago(const Duration(minutes: 6))),
        ],
      ),
      previewSession(
        id: 'l3',
        name: 'Cập nhật tài liệu hướng dẫn cài đặt và triển khai cho đội vận hành',
        project: 'Documents/personalproject/triet-digital/quan-ly',
        state: SessionState.done,
        ago: const Duration(hours: 3, minutes: 12),
      ),
      previewSession(
        id: 'l4',
        name: 'nháp',
        project: 'truongbinhtriet',
        state: SessionState.done,
        control: SessionControl.viewOnly,
        ago: const Duration(days: 2),
        takeover: const TakeoverInfo(available: true),
      ),
    ];

List<ChatMessage> longConversation() => <ChatMessage>[
      _msg('x1', ChatRole.user, 'Sửa lỗi redirect loop khi đăng nhập trên web-app và cho mình biết file nào bị ảnh hưởng, rồi chạy lại toàn bộ test của module auth.', ago: const Duration(minutes: 40)),
      _msg('x2', ChatRole.assistant, '''**Gõ `!` và `/` trong ô soạn tin:**

1. Gõ `!` ở đầu dòng để chạy lệnh shell, ví dụ `!git status --short` trong thư mục của session.
2. Gõ `/` để xem danh sách lệnh, như *`/compact`* hoặc **`/clear`** và **in đậm có *nghiêng* bên trong**.
3. File chính là `Sources/Notchling/Phone/LiveBridgeBackend.swift` (xem [tài liệu](https://example.com/docs/phone-bridge) để biết thêm chi tiết).

Một dòng rất dài không có khoảng trắng để thử cuộn: `flutter test test/chat_kit_test.dart --plain-name "composer typing a slash"` xong.''', ago: const Duration(minutes: 38)),
      _msg('x3', ChatRole.tools, '', ago: const Duration(minutes: 37), steps: const <ToolStep>[
        ToolStep(name: 'Read', summary: 'Sources/Notchling/Phone/LiveBridgeBackend.swift', output: 'final class LiveBridgeBackend {}'),
        ToolStep(name: 'Grep', summary: 'func sendMessage( in Sources/ConsoleCore/Bridge/BridgeRouter.swift'),
        ToolStep(name: 'Bash', summary: 'cd /Users/truongbinhtriet/Documents/tools/notchling-merge && swift test --filter ChatRichTests', output: 'Executed 7 tests, with 0 failures'),
      ]),
      _msg('x4', ChatRole.tools, '', ago: const Duration(minutes: 35), steps: const <ToolStep>[
        ToolStep(
          name: 'Edit',
          summary: 'Sources/Notchling/Phone/LiveBridgeBackend.swift',
          path: 'Sources/Notchling/Phone/LiveBridgeBackend.swift',
          diff: <DiffLine>[
            DiffLine(DiffKind.context, '    func sessions() async -> [Session] {'),
            DiffLine(DiffKind.removed, '        let list = await rawSessions()'),
            DiffLine(DiffKind.added, '        let list = await rawSessions()  // already enriched with activity and facts'),
          ],
        ),
      ]),
      _msg('x5', ChatRole.user, '!cd /Users/truongbinhtriet/Documents/tools/notchling-merge && git log --oneline --decorate -5', ago: const Duration(minutes: 5), kind: ChatKind.shell, output: 'bf7d9bf (HEAD -> feat/console-on-restructure) docs(console): add the Agent Console design and plans\nca05dd7 feat(console): add the visualize bundle'),
      _msg('x6', ChatRole.assistant, 'Xong. Cần bạn cho phép `git push` để mình đẩy nhánh `feat/redirect-loop-fix` lên.', ago: const Duration(minutes: 1)),
    ];
