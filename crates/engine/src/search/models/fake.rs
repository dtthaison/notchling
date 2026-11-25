use super::fetch::{Fetch, Head, Receiver};
use std::collections::HashMap;
use std::sync::Mutex;

pub enum Route {
    Bytes {
        data: Vec<u8>,
        ranges: bool,
        cut_after: Option<usize>,
    },
    Redirect(String),
    Status(u16),
}

#[derive(Default)]
pub struct FakeFetch {
    routes: Mutex<HashMap<String, Route>>,
    seen: Mutex<Vec<(String, u64)>>,
    chunk: Mutex<usize>,
}

impl FakeFetch {
    pub fn new() -> FakeFetch {
        FakeFetch {
            chunk: Mutex::new(4096),
            ..FakeFetch::default()
        }
    }

    pub fn chunk(&self, size: usize) -> &Self {
        *lock(&self.chunk) = size.max(1);
        self
    }

    pub fn serve(&self, url: &str, data: &[u8]) -> &Self {
        self.set(
            url,
            Route::Bytes {
                data: data.to_vec(),
                ranges: true,
                cut_after: None,
            },
        )
    }

    pub fn serve_without_ranges(&self, url: &str, data: &[u8]) -> &Self {
        self.set(
            url,
            Route::Bytes {
                data: data.to_vec(),
                ranges: false,
                cut_after: None,
            },
        )
    }

    pub fn cut(&self, url: &str, data: &[u8], after: usize) -> &Self {
        self.set(
            url,
            Route::Bytes {
                data: data.to_vec(),
                ranges: true,
                cut_after: Some(after),
            },
        )
    }

    pub fn redirect(&self, url: &str, to: &str) -> &Self {
        self.set(url, Route::Redirect(to.to_string()))
    }

    pub fn status(&self, url: &str, code: u16) -> &Self {
        self.set(url, Route::Status(code))
    }

    pub fn set(&self, url: &str, route: Route) -> &Self {
        lock(&self.routes).insert(url.to_string(), route);
        self
    }

    pub fn requests(&self) -> Vec<(String, u64)> {
        lock(&self.seen).clone()
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Fetch for FakeFetch {
    fn get(&self, url: &str, from: u64, rx: &mut dyn Receiver) -> Result<Head, String> {
        lock(&self.seen).push((url.to_string(), from));
        let chunk = *lock(&self.chunk);
        let routes = lock(&self.routes);
        match routes.get(url) {
            None => Ok(Head {
                status: 404,
                ..Head::default()
            }),
            Some(Route::Status(code)) => Ok(Head {
                status: *code,
                ..Head::default()
            }),
            Some(Route::Redirect(to)) => Ok(Head {
                status: 302,
                location: Some(to.clone()),
                ..Head::default()
            }),
            Some(Route::Bytes {
                data,
                ranges,
                cut_after,
            }) => {
                let partial = from > 0 && *ranges && (from as usize) <= data.len();
                let start = if partial { from as usize } else { 0 };
                let body = &data[start..];
                let head = Head {
                    status: if partial { 206 } else { 200 },
                    location: None,
                    length: Some(body.len() as u64),
                    range_start: partial.then_some(from),
                };
                rx.head(&head)?;
                let mut sent = 0;
                for piece in body.chunks(chunk) {
                    if cut_after.is_some_and(|n| sent >= n) {
                        return Err("the connection was lost".into());
                    }
                    rx.data(piece)?;
                    sent += piece.len();
                }
                Ok(head)
            }
        }
    }
}
