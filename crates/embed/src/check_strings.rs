const VIETNAMESE: [&str; 24] = [
    "Đăng nhập không được",
    "làm sao để đổi tài khoản Claude Code mà không bị đăng xuất?",
    "Tìm kiếm phiên làm việc theo dự án",
    "Giữ máy Mac luôn thức khi đang chạy tác vụ",
    "Cài đặt phím tắt cho cửa sổ trò chuyện",
    "Tại sao giới hạn tuần đã hết sớm vậy?",
    "Hiển thị lịch sử tin nhắn trong 7 ngày",
    "Xoá mật khẩu khỏi kho bí mật",
    "Ứng dụng đã cập nhật lên phiên bản mới nhất",
    "Người dùng muốn chuyển đổi giao diện tối",
    "Kết nối máy chủ qua Tailscale bị lỗi",
    "Quản lý tổ chức, máy và tài khoản",
    "Ồ, thế à! Ừ, được rồi.",
    "Đường dẫn tệp quá dài: ~/Thư mục/Tài liệu/ghi chú.md",
    "Nghiêng, đậm, gạch dưới: ỹ ỷ ỵ ữ ự ử ừ ứ",
    "Tiếng Việt có dấu hỏi, ngã, nặng, sắc, huyền",
    "Câu hỏi: tại sao phiên Codex không hiện trong danh sách?",
    "Bộ nhớ đệm đầy, hãy dọn dẹp",
    "Thông báo khi tác vụ hoàn tất",
    "Ảnh chụp màn hình bị mờ",
    "Đọc nhật ký kiểm toán tháng trước",
    "Gửi tệp đính kèm qua điện thoại",
    "Phản hồi bị treo, đang chờ xác nhận",
    "ĐĂNG NHẬP LẠI BẰNG TÀI KHOẢN KHÁC",
];

const COMBINING: [&str; 10] = [
    "Đa\u{306}ng nha\u{302}\u{323}p kho\u{302}ng đươ\u{31b}\u{323}c",
    "tie\u{302}\u{301}ng Vie\u{302}\u{323}t",
    "ta\u{300}i khoa\u{309}n",
    "nga\u{306}\u{303} ha\u{309}i",
    "chuye\u{302}\u{309}n đo\u{302}\u{309}i giao die\u{302}\u{323}n",
    "mu\u{31b}\u{300}a thu",
    "a\u{301} a\u{300} a\u{309} a\u{303} a\u{323}",
    "o\u{31b}\u{301} o\u{31b}\u{300} o\u{31b}\u{309} o\u{31b}\u{303} o\u{31b}\u{323}",
    "u\u{31b}\u{301}ng du\u{323}ng",
    "e\u{302}\u{301} e\u{302}\u{300} e\u{302}\u{309}",
];

const UNACCENTED: [&str; 12] = [
    "dang nhap khong duoc",
    "lam sao de doi tai khoan",
    "tim kiem phien lam viec",
    "giu may mac luon thuc",
    "cai dat phim tat",
    "tai sao gioi han tuan het som",
    "xoa mat khau khoi kho bi mat",
    "ket noi may chu qua tailscale",
    "thong bao khi tac vu hoan tat",
    "doc nhat ky kiem toan",
    "phan hoi bi treo",
    "DANG NHAP LAI",
];

const ENGLISH: [&str; 16] = [
    "How do I switch accounts without signing out a running session?",
    "Never refresh a token that a running session uses.",
    "Keep the Mac awake while a job runs",
    "Search sessions by project and tool",
    "The weekly limit resets on Tuesday at 10:00",
    "Show the last reply in the notch",
    "Why does the hook print nothing and exit 0?",
    "It's the user's real config; don't touch it.",
    "They'll say we've done it, I'd think.",
    "Machines, Organizations and the Secret Vault",
    "URL: https://example.com/a?b=c&d=e#f",
    "e-mail me at someone@example.com",
    "Numbers: 3.14159, 1,000,000, -42, 0x1F, 1e-9",
    "ALL CAPS HEADLINE WITH ACRONYMS LIKE RRF AND FTS5",
    "mixedCase and snake_case and kebab-case",
    "   leading and trailing spaces   ",
];

const CODE: [&str; 16] = [
    "fn main() { println!(\"hi\"); }",
    "let x: Vec<u32> = ids.iter().map(|&i| i + 1).collect();",
    "impl<'a> Iterator for Parts<'a> { type Item = &'a str; }",
    "#[derive(Debug, Clone, PartialEq)]\npub struct Config { pub hidden_size: usize }",
    "match self.act { Activation::Silu => xs.silu()?, _ => xs.gelu_erf()? }",
    "if let Some(v) = map.get(&key) { return Ok(*v); }",
    "cargo test --workspace --locked -- --ignored",
    "PYTHONPATH=tools/guard python3 -m guard lint",
    "git log --oneline -5 | grep -v '^$' && echo done",
    "for f in *.json; do jq '.queries | length' \"$f\"; done",
    "export NOTCHLING_EMBED_MODELS=\"$PWD/models\"",
    "ssh root@host 'tail -n 50 /var/log/syslog' 2>&1",
    "SELECT doc, bm25(fts) FROM fts WHERE fts MATCH 'đăng*' LIMIT 10;",
    "{\"q\": \"đăng nhập\", \"want\": [\"settings:accounts\"]}",
    "<div class=\"row\">&nbsp;</div>",
    "x <<= 2; y >>= 3; z ^= !w; a &&= b || c;",
];

const EMOJI: [&str; 12] = [
    "🎉 done",
    "👍🏽 ok",
    "👨\u{200D}👩\u{200D}👧\u{200D}👦 family",
    "🇻🇳 Việt Nam",
    "❤\u{FE0F} love it",
    "⚠\u{FE0F} warning: ⛔ stop",
    "🔥🔥🔥",
    "🙂 cảm ơn",
    "✅ tests pass, ❌ clippy fails",
    "🧑\u{200D}💻 at work",
    "日本語のテキスト",
    "Русский текст и العربية",
];

fn bases() -> Vec<&'static str> {
    let mut out = Vec::new();
    out.extend(VIETNAMESE);
    out.extend(COMBINING);
    out.extend(UNACCENTED);
    out.extend(ENGLISH);
    out.extend(CODE);
    out.extend(EMOJI);
    out
}

pub fn check_strings() -> Vec<String> {
    let bases = bases();
    let mut out: Vec<String> = bases.iter().map(|s| s.to_string()).collect();
    let n = bases.len();
    let mut i = 0;
    while out.len() < 199 {
        let a = bases[i % n];
        let b = bases[(i * 7 + 3) % n];
        let joined = match i % 4 {
            0 => format!("{a}\r\n{b}"),
            1 => format!("{a}\t{b}"),
            2 => format!("\t{a}\r\n\r\n{b}\r\n"),
            _ => format!("{} {}", a.to_uppercase(), b.to_lowercase()),
        };
        out.push(joined);
        i += 1;
    }
    let long: String = bases.join("\r\n\t").chars().cycle().take(32_000).collect();
    out.push(long);
    out
}
