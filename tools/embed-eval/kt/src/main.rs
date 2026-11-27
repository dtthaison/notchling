use kitoken::Kitoken;
use std::time::Instant;

fn rss_mb() -> f64 {
    let out = std::process::Command::new("ps").args(["-o", "rss=", "-p", &std::process::id().to_string()]).output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().parse::<f64>().unwrap_or(0.0) / 1024.0
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let t = Instant::now();
    let k = if a[1] == "convert" {
        let k = Kitoken::from_tokenizers_file(&a[2]).expect("convert");
        k.to_file(&a[3]).expect("write");
        k
    } else {
        Kitoken::from_file(&a[2]).expect("load")
    };
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    let ids = k.encode("Đăng nhập không được, dang nhap. fn main() { println!(\"hi\"); }", true).expect("enc");
    let t = Instant::now();
    let mut n = 0;
    for _ in 0..1000 {
        n += k.encode("User: làm sao để đổi tài khoản Claude Code mà không bị đăng xuất phiên đang chạy? Assistant: Never refresh a token that a running session uses.", true).unwrap().len();
    }
    println!("{} load {:.0} ms rss {:.0} MB ids {:?} ; 1000 encodes {:.1} ms ({n})", a[1], ms, rss_mb(), ids, t.elapsed().as_secs_f64() * 1000.0);
}
