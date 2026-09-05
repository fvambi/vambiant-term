//! Socket path resolution and line framing.

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use vt_proto::jsonrpc::Message;

use crate::error::IpcError;

/// Directory holding the daemon socket: `$VAMBIANT_TERM_RUNTIME` if set,
/// otherwise `$TMPDIR/vambiant-term-<uid>` (per-user and private on macOS).
pub fn runtime_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("VAMBIANT_TERM_RUNTIME") {
        return PathBuf::from(p);
    }
    let base = std::env::var_os("TMPDIR").map_or_else(|| PathBuf::from("/tmp"), PathBuf::from);
    base.join(format!("vambiant-term-{}", crate::auth::own_uid()))
}

/// Default daemon socket path.
pub fn socket_path() -> PathBuf {
    runtime_dir().join("vtermd.sock")
}

/// Create the runtime directory with mode `0700`, tightening it if it exists.
pub fn ensure_runtime_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

/// One connection: buffered line reader plus a writer clone.
#[derive(Debug)]
pub struct Framed {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
}

impl Framed {
    /// Wrap a connected stream.
    pub fn new(stream: UnixStream) -> io::Result<Self> {
        let writer = stream.try_clone()?;
        Ok(Self {
            reader: BufReader::new(stream),
            writer,
        })
    }

    /// Read the next message; `Ok(None)` on a clean EOF.
    pub fn read(&mut self) -> Result<Option<Message>, IpcError> {
        let mut line = String::new();
        let n = self.reader.read_line(&mut line)?;
        if n == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            return self.read();
        }
        serde_json::from_str::<Message>(trimmed)
            .map(Some)
            .map_err(|e| IpcError::Protocol(format!("{e}: {}", truncate(trimmed, 200))))
    }

    /// Write one message followed by a newline.
    pub fn write(&mut self, msg: &Message) -> Result<(), IpcError> {
        let mut buf = serde_json::to_vec(msg).map_err(|e| IpcError::Protocol(e.to_string()))?;
        buf.push(b'\n');
        self.writer.write_all(&buf)?;
        self.writer.flush()?;
        Ok(())
    }

    /// Shut down both directions.
    pub fn close(&self) {
        let _ = self.writer.shutdown(std::net::Shutdown::Both);
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_owned()
    } else {
        format!("{}…", &s[..max])
    }
}
