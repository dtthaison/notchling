use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const MAX_LINE: usize = 1 << 20;

const ERROR_TAIL: usize = 2000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Output {
    Line(String),
    Oversize,
    Closed(Option<String>),
}

pub trait Killer: Send + Sync {
    fn kill(&self);
}

pub trait ToolProcess: Send {
    fn write_line(&mut self, line: &str) -> io::Result<()>;
    fn killer(&self) -> Arc<dyn Killer>;
}

pub struct Spawned {
    pub process: Box<dyn ToolProcess>,
    pub output: Receiver<Output>,
}

pub trait Spawner: Send + Sync {
    fn spawn(&self, program: &str, args: &[String], folder: &str) -> Result<Spawned, String>;
}

pub fn read_lines<R: Read>(mut reader: R, max: usize, mut emit: impl FnMut(Output)) {
    let mut line: Vec<u8> = Vec::new();
    let mut skipping = false;
    let mut chunk = [0u8; 16 * 1024];
    loop {
        let n = match reader.read(&mut chunk) {
            Ok(0) => return,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return,
        };
        let mut rest = &chunk[..n];
        while !rest.is_empty() {
            match rest.iter().position(|&b| b == b'\n') {
                Some(i) => {
                    let head = &rest[..i];
                    rest = &rest[i + 1..];
                    if skipping {
                        skipping = false;
                    } else if line.len() + head.len() > max {
                        emit(Output::Oversize);
                    } else {
                        line.extend_from_slice(head);
                        emit(Output::Line(String::from_utf8_lossy(&line).into_owned()));
                    }
                    line.clear();
                }
                None => {
                    if !skipping {
                        if line.len() + rest.len() > max {
                            skipping = true;
                            line.clear();
                            emit(Output::Oversize);
                        } else {
                            line.extend_from_slice(rest);
                        }
                    }
                    rest = &[];
                }
            }
        }
    }
}

pub struct RealSpawner {
    home: PathBuf,
}

impl RealSpawner {
    pub fn new(home: &Path) -> RealSpawner {
        RealSpawner {
            home: home.to_path_buf(),
        }
    }
}

impl Spawner for RealSpawner {
    fn spawn(&self, program: &str, args: &[String], folder: &str) -> Result<Spawned, String> {
        let cwd = if folder.is_empty() {
            self.home.clone()
        } else {
            PathBuf::from(folder)
        };
        let path = [
            "/opt/homebrew/bin".to_string(),
            "/usr/local/bin".to_string(),
            self.home.join(".local/bin").to_string_lossy().into_owned(),
            std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()),
        ];
        let mut child = Command::new(program)
            .args(args)
            .current_dir(cwd)
            .env("PATH", path.join(":"))
            .env("NO_COLOR", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().ok_or("no standard output")?;
        let stderr = child.stderr.take().ok_or("no standard error")?;
        let tail = Arc::new(Mutex::new(String::new()));
        let tail_writer = tail.clone();
        let stderr_thread = std::thread::spawn(move || {
            let mut stderr = stderr;
            let mut buf = [0u8; 4096];
            while let Ok(n) = stderr.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let text = String::from_utf8_lossy(&buf[..n]).into_owned();
                let mut tail = tail_writer.lock().unwrap_or_else(|e| e.into_inner());
                tail.push_str(&text);
                let extra = tail.chars().count().saturating_sub(ERROR_TAIL);
                if extra > 0 {
                    *tail = tail.chars().skip(extra).collect();
                }
            }
        });
        let child = Arc::new(Mutex::new(child));
        let (tx, rx) = channel();
        let waiter = child.clone();
        std::thread::spawn(move || {
            read_lines(stdout, MAX_LINE, |out| {
                let _ = tx.send(out);
            });
            let _ = stderr_thread.join();
            reap(&waiter);
            let last = tail
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .map(str::to_string);
            let _ = tx.send(Output::Closed(last));
        });
        Ok(Spawned {
            process: Box::new(RealProcess { stdin, child }),
            output: rx,
        })
    }
}

fn reap(child: &Mutex<Child>) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        {
            let mut child = child.lock().unwrap_or_else(|e| e.into_inner());
            match child.try_wait() {
                Ok(Some(_)) | Err(_) => return,
                Ok(None) if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return;
                }
                Ok(None) => {}
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

struct RealProcess {
    stdin: Option<ChildStdin>,
    child: Arc<Mutex<Child>>,
}

impl ToolProcess for RealProcess {
    fn write_line(&mut self, line: &str) -> io::Result<()> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "closed"))?;
        let mut data = Vec::with_capacity(line.len() + 1);
        data.extend_from_slice(line.as_bytes());
        data.push(b'\n');
        stdin.write_all(&data)?;
        stdin.flush()
    }

    fn killer(&self) -> Arc<dyn Killer> {
        Arc::new(ChildKiller(self.child.clone()))
    }
}

struct ChildKiller(Arc<Mutex<Child>>);

impl Killer for ChildKiller {
    fn kill(&self) {
        let mut child = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let _ = child.kill();
    }
}

impl Drop for RealProcess {
    fn drop(&mut self) {
        self.stdin = None;
        ChildKiller(self.child.clone()).kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(data: &[u8], max: usize) -> Vec<Output> {
        let mut out = Vec::new();
        read_lines(data, max, |o| out.push(o));
        out
    }

    fn line(s: &str) -> Output {
        Output::Line(s.to_string())
    }

    #[test]
    fn it_cuts_lines_and_drops_the_unfinished_one() {
        assert_eq!(
            lines(b"a\n\nbb\ncc", 10),
            vec![line("a"), line(""), line("bb")]
        );
    }

    #[test]
    fn a_line_over_the_cap_is_dropped_whole_and_the_next_one_reads() {
        let mut data = vec![b'x'; 40];
        data.extend_from_slice(b"\nok\n");
        assert_eq!(lines(&data, 10), vec![Output::Oversize, line("ok")]);
        let mut at = vec![b'y'; 10];
        at.push(b'\n');
        assert_eq!(lines(&at, 10), vec![line(&"y".repeat(10))]);
    }

    #[test]
    fn a_line_over_one_mebibyte_does_not_buffer_the_rest() {
        let mut data = vec![b'x'; 3 << 20];
        data.extend_from_slice(b"\n{\"a\":1}\n");
        assert_eq!(
            lines(&data, MAX_LINE),
            vec![Output::Oversize, line("{\"a\":1}")]
        );
    }

    #[test]
    fn a_line_split_over_reads_joins_and_bad_utf8_is_replaced() {
        struct Dribble(Vec<Vec<u8>>);
        impl Read for Dribble {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                if self.0.is_empty() {
                    return Ok(0);
                }
                let part = self.0.remove(0);
                buf[..part.len()].copy_from_slice(&part);
                Ok(part.len())
            }
        }
        let mut out = Vec::new();
        read_lines(
            Dribble(vec![b"he".to_vec(), b"llo\nb".to_vec(), vec![0xff, b'\n']]),
            100,
            |o| out.push(o),
        );
        assert_eq!(out, vec![line("hello"), line("b\u{fffd}")]);
    }

    #[cfg(unix)]
    #[test]
    fn a_real_child_talks_over_pipes_and_its_end_is_reported() {
        let spawner = RealSpawner::new(&std::env::temp_dir());
        let args = vec![
            "-c".to_string(),
            "read x; echo \"got $x\"; echo oops >&2".to_string(),
        ];
        let mut spawned = spawner
            .spawn("/bin/sh", &args, &std::env::temp_dir().to_string_lossy())
            .unwrap();
        spawned.process.write_line("hi").unwrap();
        let got = spawned
            .output
            .recv_timeout(Duration::from_secs(10))
            .unwrap();
        assert_eq!(got, line("got hi"));
        let end = spawned
            .output
            .recv_timeout(Duration::from_secs(10))
            .unwrap();
        assert_eq!(end, Output::Closed(Some("oops".into())));
    }

    #[cfg(unix)]
    #[test]
    fn killing_a_real_child_ends_it() {
        let spawner = RealSpawner::new(&std::env::temp_dir());
        let args = vec!["-c".to_string(), "exec sleep 30".to_string()];
        let spawned = spawner
            .spawn("/bin/sh", &args, &std::env::temp_dir().to_string_lossy())
            .unwrap();
        spawned.process.killer().kill();
        let end = spawned
            .output
            .recv_timeout(Duration::from_secs(10))
            .unwrap();
        assert!(matches!(end, Output::Closed(_)));
        assert!(spawner.spawn("/no/such/program", &[], "").is_err());
    }
}
