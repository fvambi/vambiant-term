//! A minimal RFC 6455 WebSocket over a Unix socket, enough for
//! `codex app-server --listen unix://…` (verified transport, docs/10 §7):
//! HTTP Upgrade handshake, masked text frames out, text/ping/close frames in.
//! No extensions, no TLS — the local app-server uses neither. The server
//! side (`accept`) exists only so tests can stand in for the app-server.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

const OP_CONT: u8 = 0x0;
const OP_TEXT: u8 = 0x1;
const OP_CLOSE: u8 = 0x8;
const OP_PING: u8 = 0x9;
const OP_PONG: u8 = 0xA;

/// The sending half; shared between the reader (pongs) and any number of
/// producers.
#[derive(Debug)]
pub struct Writer {
    stream: UnixStream,
    mask_seed: u32,
    /// Clients mask, servers do not.
    mask: bool,
}

/// A connected WebSocket. Reading needs `&mut`; writing goes through the
/// shared [`Writer`] from [`WebSocket::writer`].
#[derive(Debug)]
pub struct WebSocket {
    reader: BufReader<UnixStream>,
    writer: Arc<Mutex<Writer>>,
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

fn read_headers(reader: &mut BufReader<UnixStream>) -> io::Result<Vec<String>> {
    let mut lines = Vec::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "peer closed during handshake",
            ));
        }
        if line.trim().is_empty() {
            return Ok(lines);
        }
        lines.push(line.trim_end().to_string());
        if lines.len() > 64 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "too many handshake headers",
            ));
        }
    }
}

impl WebSocket {
    /// Connect to `path` and complete the Upgrade handshake for `resource`
    /// (usually `/`).
    pub fn connect(path: &Path, resource: &str) -> io::Result<Self> {
        let stream = UnixStream::connect(path)?;
        let mut writer = stream.try_clone()?;
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let key_bytes: Vec<u8> = (0..16u32)
            .map(|i| {
                u8::try_from((seed >> (i * 4)) & 0xff).unwrap_or(0)
                    ^ u8::try_from(i).unwrap_or(0).wrapping_mul(37)
            })
            .collect();
        let key = base64(&key_bytes);
        let request = format!(
            "GET {resource} HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
        );
        writer.write_all(request.as_bytes())?;
        writer.flush()?;
        let mut reader = BufReader::new(stream);
        let lines = read_headers(&mut reader)?;
        if !lines.first().is_some_and(|l| l.contains(" 101 ")) {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionRefused,
                format!(
                    "upgrade refused: {}",
                    lines.first().map_or("", String::as_str)
                ),
            ));
        }
        Ok(Self {
            reader,
            writer: Arc::new(Mutex::new(Writer {
                stream: writer,
                mask_seed: u32::try_from(seed & 0xffff_ffff).unwrap_or(1) | 1,
                mask: true,
            })),
        })
    }

    /// Server side: read the client's Upgrade request on an accepted stream
    /// and answer 101. The accept key is not computed — our own client never
    /// checks it, and this exists for tests only.
    pub fn accept(stream: UnixStream) -> io::Result<Self> {
        let mut writer = stream.try_clone()?;
        let mut reader = BufReader::new(stream);
        let lines = read_headers(&mut reader)?;
        if !lines
            .iter()
            .any(|l| l.to_ascii_lowercase().starts_with("upgrade: websocket"))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "not a websocket upgrade",
            ));
        }
        writer.write_all(
            b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: test\r\n\r\n",
        )?;
        writer.flush()?;
        Ok(Self {
            reader,
            writer: Arc::new(Mutex::new(Writer {
                stream: writer,
                mask_seed: 1,
                mask: false,
            })),
        })
    }

    /// The shared sending half.
    pub fn writer(&self) -> Arc<Mutex<Writer>> {
        Arc::clone(&self.writer)
    }

    /// Receive the next complete text message; `Ok(None)` after a close frame
    /// or EOF. Pings are answered here.
    pub fn recv_text(&mut self) -> io::Result<Option<String>> {
        let mut message: Vec<u8> = Vec::new();
        loop {
            let mut head = [0u8; 2];
            if self.reader.read_exact(&mut head).is_err() {
                return Ok(None);
            }
            let fin = head[0] & 0x80 != 0;
            let opcode = head[0] & 0x0f;
            let masked = head[1] & 0x80 != 0;
            let mut len = u64::from(head[1] & 0x7f);
            if len == 126 {
                let mut b = [0u8; 2];
                self.reader.read_exact(&mut b)?;
                len = u64::from(u16::from_be_bytes(b));
            } else if len == 127 {
                let mut b = [0u8; 8];
                self.reader.read_exact(&mut b)?;
                len = u64::from_be_bytes(b);
            }
            let mut mask = [0u8; 4];
            if masked {
                self.reader.read_exact(&mut mask)?;
            }
            let mut payload =
                vec![0u8; usize::try_from(len).map_err(|_| io::Error::other("frame too large"))?];
            self.reader.read_exact(&mut payload)?;
            if masked {
                for (i, b) in payload.iter_mut().enumerate() {
                    *b ^= mask[i % 4];
                }
            }
            match opcode {
                OP_TEXT | OP_CONT => {
                    message.extend_from_slice(&payload);
                    if fin {
                        return Ok(Some(String::from_utf8_lossy(&message).into_owned()));
                    }
                }
                OP_PING => self
                    .writer
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .write_frame(OP_PONG, &payload)?,
                OP_CLOSE => {
                    let mut w = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
                    let _ = w.write_frame(OP_CLOSE, &[]);
                    return Ok(None);
                }
                _ => {} // pong / binary: ignore
            }
        }
    }
}

impl Writer {
    fn next_mask(&mut self) -> [u8; 4] {
        let mut x = self.mask_seed;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.mask_seed = x;
        x.to_le_bytes()
    }

    fn write_frame(&mut self, opcode: u8, payload: &[u8]) -> io::Result<()> {
        let mut head = vec![0x80 | opcode];
        let len = payload.len();
        let mask_bit = if self.mask { 0x80 } else { 0 };
        if let (true, Ok(small)) = (len < 126, u8::try_from(len)) {
            head.push(mask_bit | small);
        } else if let Ok(mid) = u16::try_from(len) {
            head.push(mask_bit | 0x7e);
            head.extend_from_slice(&mid.to_be_bytes());
        } else {
            head.push(mask_bit | 0x7f);
            head.extend_from_slice(&(len as u64).to_be_bytes());
        }
        self.stream.write_all(&head)?;
        if self.mask {
            let mask = self.next_mask();
            head.clear();
            self.stream.write_all(&mask)?;
            let masked: Vec<u8> = payload
                .iter()
                .enumerate()
                .map(|(i, b)| b ^ mask[i % 4])
                .collect();
            self.stream.write_all(&masked)?;
        } else {
            self.stream.write_all(payload)?;
        }
        self.stream.flush()
    }

    /// Send one text message.
    pub fn send_text(&mut self, text: &str) -> io::Result<()> {
        self.write_frame(OP_TEXT, text.as_bytes())
    }

    /// Send a close frame and shut the stream.
    pub fn close(&mut self) {
        let _ = self.write_frame(OP_CLOSE, &[]);
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b"a"), "YQ==");
        assert_eq!(base64(b"ab"), "YWI=");
        assert_eq!(base64(b"abc"), "YWJj");
    }

    #[test]
    fn client_and_server_exchange_masked_and_unmasked_frames() {
        let dir = std::env::temp_dir().join(format!("vt-ws-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.sock");
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        let server = std::thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            let mut ws = WebSocket::accept(s).unwrap();
            let big = "x".repeat(70_000);
            ws.writer().lock().unwrap().send_text(&big).unwrap();
            let got = ws.recv_text().unwrap().unwrap();
            ws.writer()
                .lock()
                .unwrap()
                .send_text(&format!("echo:{got}"))
                .unwrap();
            assert_eq!(ws.recv_text().unwrap(), None); // client close
        });
        let mut c = WebSocket::connect(&path, "/").unwrap();
        assert_eq!(c.recv_text().unwrap().unwrap().len(), 70_000);
        c.writer().lock().unwrap().send_text("hello").unwrap();
        assert_eq!(c.recv_text().unwrap().as_deref(), Some("echo:hello"));
        c.writer().lock().unwrap().close();
        server.join().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
