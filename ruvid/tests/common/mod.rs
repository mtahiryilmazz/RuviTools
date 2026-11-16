#![allow(dead_code)]

pub mod synthetic_media;

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Clone)]
pub struct RoutePayload {
    pub body: Vec<u8>,
    pub content_type: String,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub accept_ranges: bool,
    pub honor_range: bool,
    pub status: u16,
    pub content_disposition: Option<String>,
}

impl RoutePayload {
    pub fn bytes(body: Vec<u8>, content_type: &str) -> Self {
        Self {
            body,
            content_type: content_type.to_string(),
            etag: Some("\"v1\"".to_string()),
            last_modified: Some("Mon, 16 Nov 2026 00:00:00 GMT".to_string()),
            accept_ranges: true,
            honor_range: true,
            status: 200,
            content_disposition: None,
        }
    }

    pub fn text(text: &str, content_type: &str) -> Self {
        Self::bytes(text.as_bytes().to_vec(), content_type)
    }
}

#[derive(Clone)]
pub enum RouteEntry {
    Static(RoutePayload),
    Sequence(Arc<Mutex<Vec<RoutePayload>>>),
}

pub struct MockHttpServer {
    pub base_url: String,
    pub routes: Arc<Mutex<HashMap<String, RouteEntry>>>,
    pub requests: Arc<Mutex<Vec<String>>>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    _thread: Option<std::thread::JoinHandle<()>>,
}

impl MockHttpServer {
    pub async fn start() -> Self {
        let routes: Arc<Mutex<HashMap<String, RouteEntry>>> = Arc::new(Mutex::new(HashMap::new()));
        let requests: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let (addr_tx, addr_rx) = std::sync::mpsc::channel();
        let (tx, mut rx) = tokio::sync::oneshot::channel();

        let routes_c = routes.clone();
        let reqs_c = requests.clone();

        let thread = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("build mock server tokio runtime");
            rt.block_on(async move {
                let listener = TcpListener::bind("127.0.0.1:0")
                    .await
                    .expect("bind local mock http server");
                let addr = listener.local_addr().unwrap();
                let _ = addr_tx.send(addr);

                loop {
                    tokio::select! {
                        _ = &mut rx => break,
                        accepted = listener.accept() => {
                            let Ok((mut stream, _)) = accepted else {
                                break;
                            };
                            let routes_h = routes_c.clone();
                            let reqs_h = reqs_c.clone();
                            tokio::spawn(async move {
                                let mut buf = [0u8; 8192];
                                let n = match stream.read(&mut buf).await {
                                    Ok(n) if n > 0 => n,
                                    _ => return,
                                };
                                let req_str = String::from_utf8_lossy(&buf[..n]);
                                let mut lines = req_str.lines();
                                let first_line = lines.next().unwrap_or("");
                                let mut parts = first_line.split_whitespace();
                                let _method = parts.next().unwrap_or("GET");
                                let full_path = parts.next().unwrap_or("/");
                                let path_only = full_path.split('?').next().unwrap_or(full_path).to_string();

                                let mut range_hdr = None;
                                for line in lines {
                                    if let Some((k, v)) = line.split_once(':')
                                        && k.trim().eq_ignore_ascii_case("Range")
                                    {
                                        range_hdr = Some(v.trim().to_string());
                                    }
                                }

                                reqs_h.lock().unwrap().push(format!(
                                    "{} range={}",
                                    full_path,
                                    range_hdr.as_deref().unwrap_or("-")
                                ));

                                let payload_opt = {
                                    let guard = routes_h.lock().unwrap();
                                    guard.get(&path_only).map(|entry| match entry {
                                        RouteEntry::Static(p) => p.clone(),
                                        RouteEntry::Sequence(q) => {
                                            let mut vec = q.lock().unwrap();
                                            if vec.len() > 1 {
                                                vec.remove(0)
                                            } else {
                                                vec[0].clone()
                                            }
                                        }
                                    })
                                };

                                let Some(payload) = payload_opt else {
                                    let resp = b"HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nConnection: close\r\n\r\nNot Found";
                                    let _ = stream.write_all(resp).await;
                                    return;
                                };

                                if payload.status != 200 {
                                    let msg = format!("Error {}", payload.status);
                                    let resp = format!(
                                        "HTTP/1.1 {} Error\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                        payload.status,
                                        msg.len(),
                                        msg
                                    );
                                    let _ = stream.write_all(resp.as_bytes()).await;
                                    return;
                                }

                                let mut extra_headers = String::new();
                                if let Some(ref etag) = payload.etag {
                                    extra_headers.push_str(&format!("ETag: {etag}\r\n"));
                                }
                                if let Some(ref lm) = payload.last_modified {
                                    extra_headers.push_str(&format!("Last-Modified: {lm}\r\n"));
                                }
                                if payload.accept_ranges {
                                    extra_headers.push_str("Accept-Ranges: bytes\r\n");
                                }
                                if let Some(ref disp) = payload.content_disposition {
                                    extra_headers.push_str(&format!("Content-Disposition: {disp}\r\n"));
                                }

                                if payload.honor_range
                                    && let Some(ref rng) = range_hdr
                                    && let Some((s, e)) = parse_range(rng, payload.body.len())
                                {
                                    let slice = &payload.body[s..=e];
                                    let hdr = format!(
                                        "HTTP/1.1 206 Partial Content\r\nContent-Type: {}\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\n{}Connection: close\r\n\r\n",
                                        payload.content_type,
                                        slice.len(),
                                        s,
                                        e,
                                        payload.body.len(),
                                        extra_headers
                                    );
                                    let _ = stream.write_all(hdr.as_bytes()).await;
                                    let _ = stream.write_all(slice).await;
                                    return;
                                }

                                let hdr = format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\n{}Connection: close\r\n\r\n",
                                    payload.content_type,
                                    payload.body.len(),
                                    extra_headers
                                );
                                let _ = stream.write_all(hdr.as_bytes()).await;
                                let _ = stream.write_all(&payload.body).await;
                            });
                        }
                    }
                }
            });
        });

        let addr = addr_rx.recv().expect("receive mock server address");
        Self {
            base_url: format!("http://{addr}"),
            routes,
            requests,
            shutdown: Some(tx),
            _thread: Some(thread),
        }
    }

    pub fn set_route(&self, path: &str, payload: RoutePayload) {
        self.routes
            .lock()
            .unwrap()
            .insert(path.to_string(), RouteEntry::Static(payload));
    }

    pub fn set_sequence(&self, path: &str, sequence: Vec<RoutePayload>) {
        self.routes.lock().unwrap().insert(
            path.to_string(),
            RouteEntry::Sequence(Arc::new(Mutex::new(sequence))),
        );
    }

    pub fn request_log(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }

    pub fn clear_requests(&self) {
        self.requests.lock().unwrap().clear();
    }
}

impl Drop for MockHttpServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

fn parse_range(hdr: &str, total: usize) -> Option<(usize, usize)> {
    let rest = hdr.trim().strip_prefix("bytes=")?;
    let (s_str, e_str) = rest.split_once('-')?;
    let start = s_str.trim().parse::<usize>().ok()?;
    let end = if e_str.trim().is_empty() {
        total.saturating_sub(1)
    } else {
        e_str
            .trim()
            .parse::<usize>()
            .ok()?
            .min(total.saturating_sub(1))
    };
    if start > end || start >= total {
        None
    } else {
        Some((start, end))
    }
}

pub fn ruvid_cmd(isolated_dir: &Path) -> assert_cmd::Command {
    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("ruvid");
    cmd.env("RUVI_CONFIG_DIR", isolated_dir.join("cfg"))
        .env("RUVI_CACHE_DIR", isolated_dir.join("cache"))
        .env("RUVI_STATE_DIR", isolated_dir.join("state"))
        .env("RUVID_ALLOW_PRIVATE_NET", "1")
        .env("RUVID_RETRY_BASE_MS", "10");
    cmd
}
