---
version: alpha
name: Agent Console — Night Desk
description: Dark, dense cockpit for watching and steering Claude Code sessions on macOS and mobile.
colors:
  neutral: "#101319"
  surface: "#171B23"
  surface-raised: "#1E2330"
  border: "#262C38"
  on-surface: "#E5E9F0"
  muted: "#9BA5B5"
  primary: "#38BDF8"
  on-primary: "#04202E"
  status-input: "#F5B544"
  on-status-input: "#2A1B00"
  status-running: "#34D399"
  status-error: "#F87171"
  status-exited: "#6B7488"
typography:
  panel-title:
    fontFamily: Geist
    fontSize: 11px
    fontWeight: 600
    lineHeight: 1.3
    letterSpacing: 0.06em
  title-md:
    fontFamily: Geist
    fontSize: 15px
    fontWeight: 600
    lineHeight: 1.35
  body-md:
    fontFamily: Geist
    fontSize: 13px
    fontWeight: 400
    lineHeight: 1.5
  body-sm:
    fontFamily: Geist
    fontSize: 12px
    fontWeight: 400
    lineHeight: 1.45
  label-sm:
    fontFamily: Geist
    fontSize: 11px
    fontWeight: 500
    lineHeight: 1.3
    fontFeature: "tnum"
  code-sm:
    fontFamily: Geist Mono
    fontSize: 12px
    fontWeight: 400
    lineHeight: 1.5
rounded: { sm: 4px, md: 6px, lg: 8px }
spacing: { xs: 4px, sm: 8px, md: 12px, lg: 16px, xl: 24px }
components:
  window:
    backgroundColor: "{colors.neutral}"
    textColor: "{colors.on-surface}"
    typography: "{typography.body-md}"
  panel:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.on-surface}"
    typography: "{typography.body-md}"
  panel-header:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.muted}"
    typography: "{typography.panel-title}"
    height: 32px
    padding: 8px
  session-row:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.on-surface}"
    typography: "{typography.body-md}"
    height: 32px
    padding: 8px
  session-row-selected:
    backgroundColor: "{colors.surface-raised}"
    textColor: "{colors.on-surface}"
  subagent-row:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.muted}"
    typography: "{typography.body-sm}"
    height: 28px
  chat-user:
    backgroundColor: "{colors.surface-raised}"
    textColor: "{colors.on-surface}"
    typography: "{typography.body-md}"
    rounded: "{rounded.lg}"
    padding: 12px
  chat-assistant:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.on-surface}"
    typography: "{typography.body-md}"
  chat-tools:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.muted}"
    typography: "{typography.code-sm}"
  input:
    backgroundColor: "{colors.neutral}"
    textColor: "{colors.on-surface}"
    typography: "{typography.body-md}"
    rounded: "{rounded.md}"
    height: 36px
    padding: 8px
  button-primary:
    backgroundColor: "{colors.primary}"
    textColor: "{colors.on-primary}"
    typography: "{typography.label-sm}"
    rounded: "{rounded.md}"
    height: 28px
    padding: 12px
  button-secondary:
    backgroundColor: "{colors.surface-raised}"
    textColor: "{colors.on-surface}"
    typography: "{typography.label-sm}"
    rounded: "{rounded.md}"
    height: 28px
    padding: 12px
  badge-input:
    backgroundColor: "{colors.status-input}"
    textColor: "{colors.on-status-input}"
    typography: "{typography.label-sm}"
    rounded: "{rounded.sm}"
  status-running:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.status-running}"
    typography: "{typography.label-sm}"
  status-error:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.status-error}"
    typography: "{typography.label-sm}"
  divider:
    backgroundColor: "{colors.border}"
    height: 1px
  status-exited-dot:
    backgroundColor: "{colors.status-exited}"
    size: 8px
  tab-selected:
    backgroundColor: "{colors.surface-raised}"
    textColor: "{colors.primary}"
    typography: "{typography.label-sm}"
    rounded: "{rounded.sm}"
motion: { tier: lively }
---

## Overview

Agent Console là buồng lái của chính Triết, một developer, để theo dõi và điều khiển các session Claude Code. App chạy cả trên Mac lẫn điện thoại. Không khí tối và dày đặc, mắt phải đọc được trạng thái trong một giây.

- **Mật độ:** cao. Hàng cao 32 px, chữ thân 13 px.
- **Màu:** lấy từ palette `saas-dash-night` (uix-skill `data/palettes.csv`), không chỉnh gì. Có thêm 4 màu trạng thái, chỉ dùng đúng nghĩa trạng thái.
- **Font:** lấy từ hàng `geist-geist` (`data/fonts.csv`): Geist và Geist Mono, cả hai có tiếng Việt. Font đóng sẵn trong app.
- **Style:** `data-dense-dashboard`: hàng sát nhau, số dùng chữ số đều bề ngang, chrome mờ.

## Colors

- `neutral`: nền cửa sổ và ô nhập.
- `surface`: nền các panel.
- `surface-raised`: hàng đang chọn, bong bóng tin của người dùng, tab đang chọn, nút phụ.
- `border`: viền 1 px giữa các panel và giữa các nhóm. Không dùng làm nền.
- `on-surface`: chữ chính. Đạt 14,2:1 trên `surface`.
- `muted`: chữ phụ, tiêu đề panel, dòng subagent, dòng tóm tắt tool. Đạt 6,9:1 trên `surface`.
- `primary` (#38BDF8): **chỉ** dùng cho hành động chính (nút Gửi), vòng focus, tab visualize đang chọn và vạch chọn bên trái hàng session. Đạt 8,1:1 trên `surface`. Chữ đặt trên nền `primary` dùng `on-primary`, đạt 7,8:1.
- `status-input` (hổ phách): session cần input. Badge nền hổ phách, chữ `on-status-input`, đạt 9,2:1.
- `status-running` (xanh lá): chấm và chữ trạng thái "Đang chạy". Đạt 9,0:1.
- `status-error`: lỗi, permission bị từ chối, mất kết nối bridge. Đạt 6,2:1.
- `status-exited`: **chỉ** dùng cho chấm 8 px, vì chỉ đạt 3,7:1. Nhãn đi kèm dùng `muted`.

Trạng thái luôn đi đủ bộ màu, icon (SF Symbols trên Mac, Material Symbols trên Android) và chữ, không bao giờ chỉ có màu.

## Typography

- `panel-title`, 11 px, đậm 600, chữ in hoa, giãn 0,06em: tiêu đề panel (CHAT, SESSION, VISUALIZE).
- `title-md`, 15 px: tên session đang mở, đặt trên đầu khung chat.
- `body-md`, 13 px: tin nhắn, hàng session, ô nhập.
- `body-sm`, 12 px: dòng subagent, thời gian.
- `label-sm`, 11 px, chữ số đều bề ngang: badge, bộ đếm, tab, nút.
- `code-sm`, Geist Mono 12 px: tóm tắt tool, đường dẫn, tên nhánh, model, code trong tin nhắn.
- Chỉ tiêu đề panel viết in hoa. Còn lại viết hoa đầu câu.

## Layout

- **Bố cục Mac:** cửa sổ tối thiểu 1200×760, mặc định 1440×900.
  - Thanh trên cao 40 px, có ô ⌘K để tìm session.
  - Cột trái chiếm 44 %: khung chat ở trên (60 %), danh sách session chính ở dưới (40 %), chia bằng split kéo được.
  - Cột phải là Visualize.
  - Tỉ lệ split nhớ giữa các lần mở.
- **Lưới:** bước 8 px, khoảng cách lấy từ thang `spacing`.
- **Hàng session (32 px):** chấm trạng thái, tên, project (`muted`), badge số subagent đang chạy, thời gian cập nhật canh phải bằng chữ số đều.
- **Hàng subagent (28 px):** lùi vào 20 px. Hiện loại · mô tả · model (`code-sm`) · trạng thái.
- **Danh sách session:** nhóm theo thứ tự Cần input, Đang chạy, Xong, Đã thoát. Mỗi nhóm có tiêu đề kèm bộ đếm.
- **Chat:** tin dài tối đa 72ch. Tin của người dùng canh phải, trong bong bóng. Tin assistant canh trái, không bong bóng. Dòng tool là một dòng mono `muted` có thể mở rộng.
- **Mobile:** chia 3 tab Session → Chat → Visualize. Từ 840 dp trở lên (iPad, máy gập) thì dùng 3 panel như Mac.

## Elevation & Depth

Không đổ bóng. Các lớp tách nhau bằng bậc nền (`neutral` → `surface` → `surface-raised`) và viền 1 px `border`. Menu và popover dùng `surface-raised` kèm viền `border`.

## Shapes

- `rounded.sm` (4 px): badge, tab.
- `rounded.md` (6 px): nút, ô nhập.
- `rounded.lg` (8 px): bong bóng tin của người dùng, thẻ câu hỏi/permission, khung visualize.
- Panel và hàng danh sách để góc vuông.
- Khung điện thoại trong tab Phone giữ nguyên hình máy thật, không bo thêm.

## Components

- **Session row:**
  - Rê chuột thì nền sáng lên một bậc trong 150 ms.
  - Khi chọn: nền `surface-raised` và một vạch `primary` 2 px bên trái.
  - Bấm mũi tên để mở cây subagent; mở hay đóng đều nhớ theo session.
  - Session "Chỉ xem" có icon con mắt, kèm tooltip giải thích vì sao.
- **Thẻ câu hỏi / permission:**
  - Nằm ngay trên ô nhập, nền `surface-raised`, viền trái 2 px màu `status-input`.
  - Mỗi lựa chọn là một nút phụ, có phím số 1–9.
  - Permission có hai nút: Cho phép (nút chính) và Từ chối.
- **Ô nhập:**
  - Enter để gửi, Shift-Enter để xuống dòng.
  - Có icon micro cho voice: trên mobile dùng thật, trên Mac ẩn cho tới khi có.
  - Session chỉ xem thì ô nhập bị khoá, hiện câu "Session mở ngoài Console — chỉ xem" và nút "Mở trên Mac".
- **Nút:** nút chính nền `primary`, cả màn hình chỉ có một. Nút phụ nền `surface-raised`. Khi bấm thì co về 0,98.
- **Tab Visualize:** Phone, Web, Flow, Diagram, Entity, Chart.
  - Tự chọn tab theo loại task, có chấm nhỏ đánh dấu "gợi ý". Người dùng đổi tab thì app giữ lựa chọn đó cho session này.
- **Trạng thái của mỗi panel:**
  - Đang tải: skeleton đúng hình hàng 32 px.
  - Rỗng: một câu hướng dẫn kèm nút "Session mới".
  - Lỗi: chữ `status-error` kèm nút Thử lại.
  - Có dữ liệu.

## Motion

Motion tier: lively

- **Panel, menu, popover:** hiện ra từ scale 0,96 cộng opacity trong 200 ms, ease-out.
- **Tin nhắn mới:** trượt lên 8 px và hiện dần trong 200 ms.
- **Bộ đếm:** dùng `contentTransition(.numericText())` trên Mac và AnimatedSwitcher trên Flutter.
- **Chấm "Đang chạy":** thở bằng opacity từ 0,45 lên 1, chu kỳ 1,6 s. Đây là vòng lặp duy nhất trong app và nó mang thông tin trạng thái.
- **Đổi trạng thái:** đổi màu mượt trong 250 ms.
- **Chỉ animate `opacity` và `transform`.**
- **Reduce Motion bật:** bỏ hết chuyển động và vòng thở, chỉ giữ fade 150 ms.

## Do's and Don'ts

- **Có:** trạng thái luôn đủ màu, icon và chữ.
- **Có:** cả màn hình chỉ một nút dùng `primary`. Các nút khác dùng `surface-raised`.
- **Có:** đường dẫn, nhánh, model và tên tool viết bằng `code-sm`.
- **Không:** đổ bóng, gradient hay glow giả.
- **Không:** dùng màu trạng thái để trang trí, hay dùng `primary` cho trạng thái.
- **Không:** đặt chữ `status-exited` hay `border` lên nền, vì không đủ tương phản.
- **Không:** để dòng nào cao hơn 32 px trong danh sách session.
- **Không:** thêm vòng lặp chuyển động nào ngoài chấm "Đang chạy".
