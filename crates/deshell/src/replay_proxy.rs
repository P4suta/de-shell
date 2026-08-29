use std::io::Write as _;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub(crate) struct ReplayProxy {
    endpoint: String,
    stop: Arc<AtomicBool>,
    observations: Arc<Mutex<Vec<crate::replay::NetworkExchange>>>,
    errors: Arc<Mutex<Vec<String>>>,
    worker: Option<std::thread::JoinHandle<Result<(), String>>>,
}

impl ReplayProxy {
    pub(crate) fn start(
        replay: &crate::replay::ReplayStore,
        timeout_ms: u64,
    ) -> Result<Self, String> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .map_err(|error| format!("cannot bind replay proxy: {error}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("cannot configure replay proxy: {error}"))?;
        let address = listener
            .local_addr()
            .map_err(|error| format!("cannot resolve replay proxy address: {error}"))?;
        let endpoint = format!("http://{address}");
        let stop = Arc::new(AtomicBool::new(false));
        let observations = Arc::new(Mutex::new(Vec::new()));
        let errors = Arc::new(Mutex::new(Vec::new()));
        let worker_stop = Arc::clone(&stop);
        let worker_observations = Arc::clone(&observations);
        let worker_errors = Arc::clone(&errors);
        let replay = replay.clone();
        let socket_timeout = Duration::from_millis(timeout_ms.clamp(1, 86_400_000));
        let worker = std::thread::spawn(move || {
            while !worker_stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let result =
                            serve_one(&mut stream, &replay, &worker_observations, socket_timeout);
                        if let Err(error) = result {
                            let _ = write_error_response(&mut stream, &error);
                            worker_errors
                                .lock()
                                .map_err(|_| "replay proxy error lock was poisoned".to_owned())?
                                .push(error);
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    Err(error) => return Err(format!("replay proxy accept failed: {error}")),
                }
            }
            Ok(())
        });
        Ok(Self {
            endpoint,
            stop,
            observations,
            errors,
            worker: Some(worker),
        })
    }

    pub(crate) fn environment(&self) -> Vec<(String, String)> {
        vec![
            ("http_proxy".into(), self.endpoint.clone()),
            ("https_proxy".into(), self.endpoint.clone()),
            ("no_proxy".into(), String::new()),
        ]
    }

    pub(crate) fn finish(mut self) -> Result<Vec<crate::replay::NetworkExchange>, String> {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| "replay proxy worker panicked".to_owned())??;
        }
        let errors = self
            .errors
            .lock()
            .map_err(|_| "replay proxy error lock was poisoned".to_owned())?;
        if !errors.is_empty() {
            return Err(errors.join("; "));
        }
        self.observations
            .lock()
            .map_err(|_| "replay proxy observation lock was poisoned".to_owned())
            .map(|observations| observations.clone())
    }
}

impl Drop for ReplayProxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn serve_one(
    stream: &mut TcpStream,
    replay: &crate::replay::ReplayStore,
    observations: &Mutex<Vec<crate::replay::NetworkExchange>>,
    timeout: Duration,
) -> Result<(), String> {
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|error| format!("cannot configure replay proxy read timeout: {error}"))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|error| format!("cannot configure replay proxy write timeout: {error}"))?;
    let request = read_request(stream)?;
    let entry = replay.lookup_entry_prevalidated(&request.method, &request.uri, &request.body)?;
    let response = entry.body.to_bytes()?;
    let mut log = observations
        .lock()
        .map_err(|_| "replay proxy observation lock was poisoned".to_owned())?;
    let sequence = log.len() as u64;
    log.push(crate::replay::NetworkExchange {
        sequence,
        method: request.method,
        uri: request.uri,
        request_body_sha256: crate::digest::sha256(&request.body),
        status: entry.status,
        response_body_sha256: crate::digest::sha256(&response),
    });
    drop(log);

    write!(stream, "HTTP/1.1 {} Replay\r\n", entry.status)
        .map_err(|error| format!("cannot write replay response status: {error}"))?;
    for header in &entry.headers {
        if !matches!(
            header.name.to_ascii_lowercase().as_str(),
            "connection" | "content-length" | "proxy-connection" | "transfer-encoding"
        ) {
            write!(stream, "{}: {}\r\n", header.name, header.value)
                .map_err(|error| format!("cannot write replay response header: {error}"))?;
        }
    }
    write!(
        stream,
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        response.len()
    )
    .map_err(|error| format!("cannot write replay response framing: {error}"))?;
    stream
        .write_all(&response)
        .and_then(|()| stream.flush())
        .map_err(|error| format!("cannot write replay response body: {error}"))?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(|error| format!("cannot close replay response cleanly: {error}"))
}

#[derive(Debug)]
struct HttpRequest {
    method: String,
    uri: String,
    body: Vec<u8>,
}

fn read_request(stream: &mut impl std::io::Read) -> Result<HttpRequest, String> {
    const MAX_REQUEST: usize = 1024 * 1024;
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 8192];
    let header_end = loop {
        let count = stream
            .read(&mut chunk)
            .map_err(|error| format!("cannot read replay request: {error}"))?;
        if count == 0 {
            return Err("replay request ended before its headers".into());
        }
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.len() > MAX_REQUEST {
            return Err("replay request exceeds 1 MiB".into());
        }
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let headers = std::str::from_utf8(&bytes[..header_end])
        .map_err(|_| "replay request headers are not UTF-8")?;
    let mut lines = headers.split("\r\n");
    let request_line = lines.next().ok_or("replay request line is missing")?;
    let mut request_parts = request_line.split_ascii_whitespace();
    let method = request_parts
        .next()
        .ok_or("replay request method is missing")?
        .to_ascii_uppercase();
    let target = request_parts
        .next()
        .ok_or("replay request target is missing")?
        .to_owned();
    let version = request_parts
        .next()
        .ok_or("replay request version is missing")?;
    if request_parts.next().is_some() || !version.starts_with("HTTP/1.") {
        return Err("replay request line is malformed".into());
    }
    if method == "CONNECT" {
        return Err("HTTPS CONNECT replay is unavailable".into());
    }
    let mut host = None::<String>;
    let mut content_length = None::<usize>;
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line
            .split_once(':')
            .ok_or("replay request contains a malformed header")?;
        if name.eq_ignore_ascii_case("host") {
            if host.is_some() {
                return Err("replay request contains duplicate Host".into());
            }
            let value = value.trim();
            if value.is_empty() {
                return Err("replay request Host is empty".into());
            }
            host = Some(value.to_owned());
        } else if name.eq_ignore_ascii_case("content-length") {
            if content_length.is_some() {
                return Err("replay request contains duplicate Content-Length".into());
            }
            content_length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| "replay request Content-Length is invalid")?,
            );
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err("chunked replay requests are unavailable".into());
        }
    }
    let content_length = content_length.unwrap_or(0);
    if header_end + content_length > MAX_REQUEST {
        return Err("replay request exceeds 1 MiB".into());
    }
    while bytes.len() < header_end + content_length {
        let count = stream
            .read(&mut chunk)
            .map_err(|error| format!("cannot read replay request body: {error}"))?;
        if count == 0 {
            return Err("replay request body ended early".into());
        }
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.len() > MAX_REQUEST {
            return Err("replay request exceeds 1 MiB".into());
        }
    }
    if bytes.len() > header_end + content_length {
        return Err("replay request contains trailing bytes after its declared body".into());
    }
    let uri = if target.starts_with("http://") || target.starts_with("https://") {
        target
    } else if target.starts_with('/') {
        format!(
            "http://{}{}",
            host.ok_or("origin-form replay request omitted Host")?,
            target
        )
    } else {
        return Err("replay request target form is unsupported".into());
    };
    Ok(HttpRequest {
        method,
        uri,
        body: bytes[header_end..header_end + content_length].to_vec(),
    })
}

fn write_error_response(stream: &mut impl std::io::Write, error: &str) -> Result<(), String> {
    let body = error.as_bytes();
    write!(
        stream,
        "HTTP/1.1 502 Replay Error\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .and_then(|()| stream.write_all(body))
    .and_then(|()| stream.flush())
    .map_err(|write_error| format!("cannot write replay error response: {write_error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read as _;

    fn parse(raw: &[u8]) -> Result<HttpRequest, String> {
        read_request(&mut std::io::Cursor::new(raw))
    }

    #[test]
    fn proxy_replays_exact_http_and_records_body_digests() {
        let replay = crate::replay::ReplayStore {
            schema_version: 1,
            entries: vec![crate::replay::ReplayEntry {
                method: "POST".into(),
                uri: "http://example.test/data".into(),
                request_body_sha256: crate::digest::sha256(b"request"),
                status: 201,
                headers: vec![],
                body: crate::ir::SourceBytes::from_bytes(b"response"),
            }],
        };
        let proxy = ReplayProxy::start(&replay, 5_000).unwrap();
        let endpoint = proxy.endpoint.strip_prefix("http://").unwrap();
        let mut stream = TcpStream::connect(endpoint).unwrap();
        stream
            .write_all(
                b"POST http://example.test/data HTTP/1.1\r\nHost: example.test\r\nContent-Length: 7\r\n\r\nrequest",
            )
            .unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).unwrap();
        assert!(response.starts_with(b"HTTP/1.1 201 Replay\r\n"));
        assert!(response.ends_with(b"response"));
        let observations = proxy.finish().unwrap();
        assert_eq!(observations.len(), 1);
        assert_eq!(
            observations[0].request_body_sha256,
            crate::digest::sha256(b"request")
        );
        assert_eq!(
            observations[0].response_body_sha256,
            crate::digest::sha256(b"response")
        );
    }

    #[test]
    fn request_parser_accepts_only_unambiguous_absolute_or_origin_form_http() {
        let absolute = parse(
            b"post http://example.test/data HTTP/1.1\r\nHost: ignored.test\r\nContent-Length: 3\r\n\r\nraw",
        )
        .unwrap();
        assert_eq!(absolute.method, "POST");
        assert_eq!(absolute.uri, "http://example.test/data");
        assert_eq!(absolute.body, b"raw");

        let origin = parse(b"GET /path?q=1 HTTP/1.0\r\nHost: example.test:8080\r\n\r\n").unwrap();
        assert_eq!(origin.method, "GET");
        assert_eq!(origin.uri, "http://example.test:8080/path?q=1");
        assert!(origin.body.is_empty());

        let cases: &[(&[u8], &str)] = &[
            (b"", "ended before its headers"),
            (b"\xff\r\n\r\n", "not UTF-8"),
            (b"\r\n\r\n", "method is missing"),
            (b"GET\r\n\r\n", "target is missing"),
            (b"GET /\r\n\r\n", "version is missing"),
            (b"GET / HTTP/2\r\n\r\n", "malformed"),
            (b"GET / HTTP/1.1 extra\r\n\r\n", "malformed"),
            (b"CONNECT example.test:443 HTTP/1.1\r\n\r\n", "CONNECT"),
            (b"GET / HTTP/1.1\r\nmalformed\r\n\r\n", "malformed header"),
            (
                b"GET / HTTP/1.1\r\nContent-Length: nope\r\n\r\n",
                "Content-Length is invalid",
            ),
            (
                b"GET / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n",
                "chunked",
            ),
            (b"GET / HTTP/1.1\r\n\r\n", "omitted Host"),
            (
                b"GET relative HTTP/1.1\r\nHost: example.test\r\n\r\n",
                "target form",
            ),
            (
                b"GET / HTTP/1.1\r\nHost: one\r\nHost: two\r\n\r\n",
                "duplicate Host",
            ),
            (
                b"POST / HTTP/1.1\r\nHost: one\r\nContent-Length: 1\r\nContent-Length: 1\r\n\r\nx",
                "duplicate Content-Length",
            ),
            (
                b"POST / HTTP/1.1\r\nHost: one\r\nContent-Length: 1\r\n\r\nxy",
                "trailing bytes",
            ),
            (
                b"POST / HTTP/1.1\r\nHost: one\r\nContent-Length: 2\r\n\r\nx",
                "body ended early",
            ),
        ];
        for (raw, expected) in cases {
            let error = match parse(raw) {
                Ok(_) => panic!("accepted malformed request: {raw:?}"),
                Err(error) => error,
            };
            assert!(error.contains(expected), "missing {expected:?} in {error}");
        }

        let oversized = format!(
            "POST / HTTP/1.1\r\nHost: example.test\r\nContent-Length: {}\r\n\r\n",
            1024 * 1024
        );
        assert!(
            parse(oversized.as_bytes())
                .unwrap_err()
                .contains("exceeds 1 MiB")
        );
    }

    #[test]
    fn replay_error_response_is_bounded_and_self_framing() {
        let mut output = Vec::new();
        write_error_response(&mut output, "replay miss").unwrap();
        assert_eq!(
            output,
            b"HTTP/1.1 502 Replay Error\r\nContent-Length: 11\r\nConnection: close\r\n\r\nreplay miss"
        );
    }
}
