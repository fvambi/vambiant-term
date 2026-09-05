//! A deliberately small HTTP/1.1 server on the loopback interface.
//!
//! Two consumers: Claude Code's `http` hook handlers and status line relay
//! (POST JSON, expect JSON), and the token-authenticated loopback API
//! (ADR-0004). It speaks exactly what those need — request line, headers,
//! `Content-Length` bodies, one response — and nothing else: no keep-alive
//! pipelining, no chunked requests, no TLS. Bodies are capped so a hostile
//! local process cannot exhaust memory.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

/// Largest request body accepted (Claude hook payloads are ≤ a few hundred KB).
pub const MAX_BODY: usize = 8 * 1024 * 1024;

/// A parsed request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpRequest {
    /// `GET`, `POST`, …
    pub method: String,
    /// Path and query as sent.
    pub path: String,
    /// Headers, names lower-cased.
    pub headers: Vec<(String, String)>,
    /// Body bytes.
    pub body: Vec<u8>,
}

impl HttpRequest {
    /// First header value with this (case-insensitive) name.
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// A response to write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpResponse {
    /// Status code.
    pub status: u16,
    /// Content type.
    pub content_type: &'static str,
    /// Body.
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// `200` with a JSON body.
    pub fn json(value: &serde_json::Value) -> Self {
        Self {
            status: 200,
            content_type: "application/json",
            body: value.to_string().into_bytes(),
        }
    }

    /// Any status with a plain-text body.
    pub fn text(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            content_type: "text/plain; charset=utf-8",
            body: body.into().into_bytes(),
        }
    }
}

/// Request handler shared by every connection.
pub trait HttpHandler: Send + Sync + 'static {
    /// Produce the response. Blocking here is allowed and expected: a hook
    /// request may be held while a human decides.
    fn handle(&self, request: HttpRequest) -> HttpResponse;
}

/// A running loopback listener.
#[derive(Debug)]
pub struct HttpServer {
    port: u16,
}

impl HttpServer {
    /// Bind `127.0.0.1:port` (`0` = ephemeral) and serve on background threads.
    pub fn start(port: u16, handler: Arc<dyn HttpHandler>) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        let port = listener.local_addr()?.port();
        thread::Builder::new()
            .name("vt-http-accept".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    let Ok(stream) = stream else { break };
                    let handler = Arc::clone(&handler);
                    thread::Builder::new()
                        .name("vt-http-conn".into())
                        .spawn(move || serve(stream, &*handler))
                        .ok();
                }
            })?;
        Ok(Self { port })
    }

    /// Bound port.
    pub fn port(&self) -> u16 {
        self.port
    }
}

fn serve(mut stream: TcpStream, handler: &dyn HttpHandler) {
    let response = match read_request(&mut stream) {
        Ok(req) => handler.handle(req),
        Err(status) => HttpResponse::text(status, "bad request"),
    };
    let _ = write_response(&mut stream, &response);
    let _ = stream.shutdown(std::net::Shutdown::Both);
}

fn read_request(stream: &mut TcpStream) -> Result<HttpRequest, u16> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|_| 400_u16)?;
    let mut parts = line.trim_end().split(' ');
    let method = parts
        .next()
        .filter(|m| !m.is_empty())
        .ok_or(400_u16)?
        .to_string();
    let path = parts.next().ok_or(400_u16)?.to_string();
    let mut headers = Vec::new();
    let mut content_length = 0usize;
    loop {
        line.clear();
        reader.read_line(&mut line).map_err(|_| 400_u16)?;
        let l = line.trim_end();
        if l.is_empty() {
            break;
        }
        let (name, value) = l.split_once(':').ok_or(400_u16)?;
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim().to_string();
        if name == "content-length" {
            content_length = value.parse().map_err(|_| 400_u16)?;
            if content_length > MAX_BODY {
                return Err(413_u16);
            }
        }
        headers.push((name, value));
    }
    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body).map_err(|_| 400_u16)?;
    Ok(HttpRequest {
        method,
        path,
        headers,
        body,
    })
}

fn write_response(stream: &mut TcpStream, resp: &HttpResponse) -> std::io::Result<()> {
    let reason = match resp.status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        _ => "Status",
    };
    let head = format!(
        "HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        resp.status,
        resp.content_type,
        resp.body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(&resp.body)?;
    stream.flush()
}

/// Minimal client for tests and the `vterm` relay: one request, one response.
pub fn post_json(port: u16, path: &str, body: &[u8]) -> std::io::Result<(u16, Vec<u8>)> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))?;
    let head = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let status: u16 = line
        .split(' ')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let mut len = 0usize;
    loop {
        line.clear();
        reader.read_line(&mut line)?;
        let l = line.trim_end();
        if l.is_empty() {
            break;
        }
        if let Some((n, v)) = l.split_once(':')
            && n.trim().eq_ignore_ascii_case("content-length")
        {
            len = v.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    Ok((status, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Echo;
    impl HttpHandler for Echo {
        fn handle(&self, req: HttpRequest) -> HttpResponse {
            if req.method != "POST" {
                return HttpResponse::text(405, "POST only");
            }
            let v: serde_json::Value =
                serde_json::from_slice(&req.body).unwrap_or(serde_json::Value::Null);
            HttpResponse::json(
                &serde_json::json!({ "path": req.path, "got": v, "ct": req.header("Content-Type") }),
            )
        }
    }

    #[test]
    fn round_trip_and_errors() {
        let server = HttpServer::start(0, Arc::new(Echo)).unwrap();
        let (status, body) = post_json(server.port(), "/hook/tok", br#"{"a":1}"#).unwrap();
        assert_eq!(status, 200);
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["path"], "/hook/tok");
        assert_eq!(v["got"]["a"], 1);
        assert_eq!(v["ct"], "application/json");

        // Oversized bodies are refused before being read.
        let mut s = TcpStream::connect(("127.0.0.1", server.port())).unwrap();
        s.write_all(
            format!(
                "POST /x HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
                MAX_BODY + 1
            )
            .as_bytes(),
        )
        .unwrap();
        let mut line = String::new();
        BufReader::new(s).read_line(&mut line).unwrap();
        assert!(line.contains("413"), "{line}");
    }
}
