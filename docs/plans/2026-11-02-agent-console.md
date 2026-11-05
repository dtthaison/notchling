# Agent Console: app macOS + app mobile (iOS, Android)

Nhánh: `feat/agent-console`. Trạng thái: đã duyệt, P0 xong (xem `2026-11-02-p0-spike.md`), đang làm P1.

## Mục tiêu

Một bảng điều khiển cho mọi session Claude Code trên Mac:

1. Danh sách session theo trạng thái **Đang chạy / Cần input / Xong**, tách agent chính và subagent.
2. Panel visualize bên phải: điện thoại (simulator/emulator), web, chart, flow code, diagram, entity, tuỳ loại task.
3. Layout Mac: trên trái là chat của session đang chọn, dưới trái là danh sách session chính, phải là visualize.

App mobile làm được y như vậy, cộng thêm:

- nhắn từ điện thoại vào đúng session đang chọn;
- nhập bằng giọng nói, model nhúng thẳng trong app.

## Ràng buộc

App Console là **một app macOS riêng** nằm cùng repo, có bundle id riêng (`local.agentconsole`), thư mục riêng và script build riêng. App này **không sửa** source, file build, hook hay cài đặt của Notchling. Bảng dưới chỉ là nơi tham khảo cách làm; Console viết lại phần đọc dữ liệu trong code của nó.

## Notchling làm thế nào (chỉ để tham khảo)

| Cần | Đã có | Ở đâu |
| --- | --- | --- |
| Danh sách session sống | `~/.claude/sessions/<pid>.json`, `~/.claude/jobs/<id>/state.json` | `AgentMonitor.swift:134-182` |
| Trạng thái | waiting → cần input, busy/shell → đang chạy, còn lại → xong; session đã thoát → "Exited" | `AgentMonitor.swift:98-158` |
| Chat | đọc 2 MB cuối transcript, user/assistant + tóm tắt tool, câu hỏi đang chờ | `TranscriptReader.swift` |
| Trả lời permission / câu hỏi | hook `Notchling --hook` qua file requests/responses | `HookBridge.swift` |
| Nhắn vào job nền | `claude attach` trong pty, bracketed paste | `ReplySender.swift` |

Chưa có:

- **Subagent:** Notchling đang bỏ qua dòng `isSidechain`. Dữ liệu thì có sẵn: `<session>/subagents/agent-<id>.jsonl` và `agent-<id>.meta.json`, với các trường `agentType`, `description`, `model`, `toolUseId` (nối về lời gọi Agent trong session cha), `spawnDepth`, `finished`.
- **Nhắn vào session tương tác** (terminal, Claude desktop): hiện chỉ job nền mới nhận được tin nhắn. Mỗi file session có `messagingSocketPath` (`/tmp/cc-socks/<pid>.sock`), `peerProtocol: 1` và `peerFeatures`. Đây là kênh nội bộ, chưa có tài liệu, phải spike trước.
- **Server mạng:** chưa có. Listener duy nhất là redirect OAuth tạm thời.

## Kiến trúc

```
~/.claude (sessions, jobs, projects/*.jsonl, subagents/)   ← chỉ đọc
  -> SessionStore (Swift, viết mới trong Console/, theo cách AgentMonitor + TranscriptReader đọc, thêm SubagentReader)
  -> Agent Console.app (SwiftUI, macOS)      ── dùng trực tiếp
  -> Bridge (NWListener: HTTP + WebSocket)   ── cho mobile
       -> app mobile Flutter (iOS + Android)
  -> VizEngine: nhận diện loại task -> VizSpec (JSON)
       -> viz bundle (HTML + JS: mermaid, chart) render trong WKWebView (Mac) và webview_flutter (mobile)
```

- **Mac:** `Agent Console.app`, một app cửa sổ bình thường (có Dock icon), chạy độc lập với Notchling. Hai app cùng đọc `~/.claude`, nhưng chỉ Console mở bridge.
- **Permission và câu hỏi:** hook `Notchling --hook` đang được đăng ký trong `~/.claude/settings.json`. Nếu Console đăng ký thêm hook của nó, hai app sẽ tranh nhau trả lời. Hướng xử lý nằm ở Quyết định cần chốt.
- **Mobile:** viết bằng Flutter để một codebase chạy cả iOS và Android, giống Maneki.
- **Visualize:** dùng chung một bundle HTML/JS cho cả hai app, viết một lần.

### Mô hình dữ liệu

```
Session { id, name, project, cwd, entrypoint (terminal | desktop | job | sdk), state (running | needsInput | done | exited),
          since, lastActivity, pending (question | permission)?, subagents: [Subagent] }
Subagent { id, type, description, model, depth, parentToolUseId, finished, lastActivity }
```

- **Đang chạy:** busy/shell, hoặc có subagent chưa `finished`.
- **Cần input:** waiting, hoặc có hook request đang chờ.
- **Xong:** idle hoặc exited. Session xong quá 24 h thì ẩn, giống Notchling.

### Giao diện Mac

```
┌──────────────────────────────┬─────────────────────────────────┐
│ Chat: session đang chọn       │ Visualize  [Phone|Web|Flow|      │
│ (tin nhắn, tool, câu hỏi,     │            Diagram|Entity|Chart]│
│  permission, ô nhập + gửi)    │                                 │
├──────────────────────────────┤  tự chọn tab theo loại task,    │
│ Session chính                 │  người dùng đổi được             │
│ ● Cần input (2)               │                                 │
│ ● Đang chạy (3)  ▸ 2 subagent │                                 │
│ ● Xong (5)                    │                                 │
└──────────────────────────────┴─────────────────────────────────┘
```

- Danh sách dưới trái chỉ hiện session chính. Bấm mũi tên để mở cây subagent: loại, mô tả, model, trạng thái. Bấm vào một subagent thì khung chat hiện transcript của subagent đó, ở chế độ chỉ xem.
- Thiết kế theo uix-skill. Bước `init` gồm 3 câu hỏi và 3 hướng để chọn, sau đó ghi `DESIGN.md`. Có đủ 4 trạng thái loading / rỗng / lỗi / có dữ liệu. Kiểm tra cả chế độ sáng và tối.

### Visualize: nhận diện và nguồn

| Tab | Khi nào tự bật | Nguồn |
| --- | --- | --- |
| Phone | cwd có `pubspec.yaml`, `*.xcodeproj` hoặc `android/`; transcript có `simctl`/`adb` | Mac: ScreenCaptureKit quay cửa sổ DeviceHub (`com.apple.dt.Devices`) hoặc Simulator, khoảng 29 fps; cửa sổ phải đang hiện. Mobile: frame JPEG qua WebSocket |
| Web | transcript có `localhost:<port>`, hoặc có `.claude/launch.json` hoặc script dev | Mac: WKWebView trỏ URL. Mobile: ảnh chụp WKWebView gửi qua bridge (v1) |
| Flow | luôn có | timeline tool call (Read/Edit/Bash/Agent) dạng flow; các file đã sửa và `git diff` trong cwd |
| Diagram | assistant có code block ```mermaid | render bằng mermaid trong bundle |
| Entity | cwd có prisma, drift, migration SQL, entity TypeORM | parse schema ra `erDiagram` |
| Chart | luôn có | token và chi phí theo thời gian (dùng lại CostTracker), số tool call, test pass/fail nếu có |

### Bridge (Mac ↔ điện thoại)

- `NWListener` mở HTTP + WebSocket.
  - `GET /sessions`
  - `GET /sessions/:id/chat`
  - `GET /sessions/:id/viz`
  - `POST /sessions/:id/message`
  - `POST /requests/:id/answer`
  - WS `/events` đẩy thay đổi trạng thái, tin nhắn mới và frame Phone.
- **Ghép đôi:** Mac hiện QR chứa host, port và token; Mac phải bấm duyệt máy mới. Trong nhà tìm nhau qua Bonjour; ra ngoài thì dùng Tailscale (miễn phí, không cần server riêng).
- **Bảo mật:** gửi tin vào session nghĩa là chạy lệnh trên máy. Vì vậy:
  - bắt buộc token, hỗ trợ thu hồi;
  - chỉ nghe trên LAN và interface Tailscale;
  - ghi log mọi lệnh gửi từ xa;
  - mặc định tắt cho tới khi ghép đôi.

### Mobile

- **Màn hình:** Session (cây chính → subagent) → Chat → Visualize. Trên iPad và máy gập, ba panel giống Mac.
- **Nhắn đúng session:** mọi tin nhắn đều mang `sessionId`. Bridge chọn kênh theo loại session:

  | Loại | Kênh gửi |
  | --- | --- |
  | job | `claude attach` |
  | session do Console mở (nút "Session mới") | gõ vào pty của Console |
  | câu hỏi, permission | `HookBridge` responses |

  Session mở ở chỗ khác (app Claude desktop, terminal VS Code) không nhận được tin: app ghi rõ "Chỉ xem", vẫn trả lời được permission và câu hỏi, và có nút "Mở trên Mac". Socket peer thì không dùng: Claude coi tin qua đó là của session khác, không phải của người dùng.
- **Giọng nói, nhúng model (chốt sau P0b):**
  - Whisper small q5_1 (181 MiB) chạy bằng whisper.cpp (Core ML trên iOS), đóng sẵn trong app.
  - Câu gợi ý thuật ngữ (initial prompt) lấy theo session: tên file, nhánh, thuật ngữ trong tin gần đây, cộng một bảng cố định. Có lớp chặn lặp chữ và cắt khoảng lặng.
  - Voice chỉ đọc chính tả vào ô nhập để sửa rồi gửi. Lúc đo, nhận dạng đúng khoảng một nửa (45 % từ sai) nên không gửi thẳng.
  - Đã loại: sherpa tiếng Việt của Maneki (60 % từ sai), có hotwords cũng vậy; Apple on-device (67 %); PhoWhisper (73 %).
- **Thông báo:** khi app đang kết nối thì báo local lúc session cần input. Push khi app tắt cần APNs/FCM, để giai đoạn sau.

## Cấu trúc repo

```
Sources/            Notchling (giữ nguyên, không sửa)
Console/            Agent Console.app: Sources/, Resources/, Info.plist, build.sh (build ra Console/build/)
  Sources/Store/    SessionStore, SubagentReader, TranscriptReader của Console
  Sources/Bridge/   HTTP + WebSocket
  Sources/UI/       các panel
  Tests/            test với transcript mẫu
viz/                bundle HTML/JS dùng chung (mermaid, chart), copy vào Console và mobile lúc build
mobile/             app Flutter (iOS + Android)
docs/plans/         plan này
```

`build.sh` của Notchling chỉ compile `Sources/*.swift`, nên khi thêm `Console/` thì Notchling vẫn build y như cũ.

## Các giai đoạn

| # | Việc | Xong khi |
| --- | --- | --- |
| P0 | Spike 1–2 ngày: (a) gửi tin vào session terminal và desktop qua socket peer; (b) đo độ đúng sherpa tiếng Việt trên 20 câu lệnh dev trộn tiếng Anh; (c) ScreenCaptureKit quay Simulator ở 30 fps | báo cáo kết quả và chốt kênh gửi, chốt model |
| P1 | Khung app `Console/` + `build.sh` riêng; SessionStore + SubagentReader, có test với transcript mẫu | đọc đúng cây chính/subagent và 3 trạng thái; `./build.sh` của Notchling vẫn ra bản giống hệt |
| P2 | Giao diện Agent Console: chạy uix init, layout 3 panel, chat, trả lời câu hỏi/permission, nhắn | chạy được với session thật, có GIF duyệt |
| P3 | Visualize v1: Phone, Web, Flow, Diagram, Entity, Chart | mỗi tab có một task thật làm mẫu |
| P4 | Bridge: API, WebSocket, ghép đôi QR, Bonjour | gọi được bằng curl và wscat từ máy khác |
| P5 | App Flutter: ghép đôi, danh sách, chat, visualize, nhắn đúng session | chạy trên sim iPhone và Android emulator (từng máy một) |
| P6 | Voice nhúng model + bảng thuật ngữ | nói 20 câu mẫu, đúng ≥ 90 % |
| P7 | Hoàn thiện: thông báo, tự nối lại, review bảo mật | Triết duyệt |

## Rủi ro

- **Socket peer** là giao thức nội bộ của Claude Code, có thể đổi giữa các bản. Nếu không dùng được thì session tương tác chỉ nhắn được khi chạy dạng job nền. Vì vậy P0 làm trước.
- **Tiếng Việt trộn thuật ngữ tiếng Anh** có thể làm model tiếng Việt nhận sai. Nếu P0 đo thấp thì thử Whisper small bản lượng tử hoá, khoảng 180 MB.
- **Visualize web trên mobile** v1 chỉ là ảnh chụp, không tương tác. Muốn tương tác thì cần reverse proxy qua bridge, để sau.

## Quyết định cần chốt

1. **Trả lời permission và câu hỏi từ Console:**
   - (đề xuất) Không đăng ký hook mới. Console đọc `requests/` và ghi `responses/` trong `~/Library/Application Support/Notchling/`, đúng giao thức file Notchling đang dùng, và không sửa code Notchling. Cách này chỉ chạy khi Notchling đang mở; khi Notchling tắt thì Console chỉ hiện "đang chờ trên terminal".
   - Hoặc Console có hook riêng. Cách này phải sửa `~/.claude/settings.json` và sẽ tranh với hook của Notchling.
2. **Kết nối từ xa:** dùng Tailscale (đề xuất, miễn phí, mã hoá sẵn), hay Cloudflare Tunnel?
