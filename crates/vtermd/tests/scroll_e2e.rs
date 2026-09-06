//! The viewport is daemon state: `session.scroll` moves it, deltas carry
//! its absolute position, `session.text` reads scrollback by absolute row,
//! and typing snaps back to the live end.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use base64::Engine as _;
use vt_ipc::Client;
use vt_proto::session::{NewSession, OutputDelta, SessionInfo, method};

struct Daemon {
    child: Child,
    socket: PathBuf,
    dir: PathBuf,
}

impl Daemon {
    fn start() -> Self {
        let dir = std::env::temp_dir().join(format!("vtermd-scroll-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("vtermd.sock");
        let child = Command::new(env!("CARGO_BIN_EXE_vtermd"))
            .arg("--socket")
            .arg(&socket)
            .env("VAMBIANT_TERM_STATE", dir.join("state"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn vtermd");
        let start = Instant::now();
        while !socket.exists() {
            assert!(start.elapsed() < Duration::from_secs(10), "no socket");
            std::thread::sleep(Duration::from_millis(20));
        }
        Self { child, socket, dir }
    }

    fn client(&self) -> Client {
        let start = Instant::now();
        loop {
            if let Ok(c) = Client::connect(&self.socket) {
                return c;
            }
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = Command::new("pkill")
            .arg("-f")
            .arg(self.dir.display().to_string())
            .status();
    }
}

fn row_text(d: &OutputDelta, row: u16) -> String {
    d.lines
        .iter()
        .find(|l| l.row == row)
        .map(|l| {
            l.cells
                .iter()
                .map(|c| c.c)
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .unwrap_or_default()
}

#[test]
fn viewport_scrolls_and_text_reads_scrollback_by_absolute_row() {
    let daemon = Daemon::start();
    let mut c = daemon.client();
    let script = "i=1; while [ $i -le 40 ]; do echo line$i; i=$((i+1)); done; sleep 30";
    let req = NewSession {
        name: Some("scroll".into()),
        argv: vec!["/bin/sh".into(), "-c".into(), script.into()],
        cwd: Some(std::env::temp_dir()),
        size: Some((20, 6)),
        ..Default::default()
    };
    let info: SessionInfo = serde_json::from_value(
        c.call(method::SESSION_NEW, serde_json::to_value(req).ok())
            .unwrap(),
    )
    .unwrap();
    let id = serde_json::json!({ "id": info.id.0 });

    // Wait for the output to land: the live grid ends with line40.
    let start = Instant::now();
    loop {
        let logs = c.call(method::SESSION_LOGS, Some(id.clone())).unwrap();
        if logs["text"].as_str().unwrap_or("").contains("line40") {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "no output: {logs}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    // At the live end: 40 lines plus the cursor row, six visible.
    let bottom: OutputDelta =
        serde_json::from_value(c.call(method::SESSION_ATTACH, Some(id.clone())).unwrap()).unwrap();
    assert_eq!(bottom.total, 41, "{bottom:?}");
    assert_eq!(bottom.top, 35);

    // Scroll to the top: the reply and the next snapshot agree, and the
    // first visible row is the oldest line.
    let r = c
        .call(
            method::SESSION_SCROLL,
            Some(serde_json::json!({ "id": info.id.0, "to": "top" })),
        )
        .unwrap();
    assert_eq!(r["top"], 0);
    assert_eq!(r["total"], 41);
    let top: OutputDelta =
        serde_json::from_value(c.call(method::SESSION_ATTACH, Some(id.clone())).unwrap()).unwrap();
    assert_eq!(top.top, 0);
    assert_eq!(row_text(&top, 0), "line1");

    // Relative and absolute moves.
    let r = c
        .call(
            method::SESSION_SCROLL,
            Some(serde_json::json!({ "id": info.id.0, "to": "lines", "n": 3 })),
        )
        .unwrap();
    assert_eq!(r["top"], 3);
    let r = c
        .call(
            method::SESSION_SCROLL,
            Some(serde_json::json!({ "id": info.id.0, "to": "row", "n": 10 })),
        )
        .unwrap();
    assert_eq!(r["top"], 10);
    let err = c
        .call(
            method::SESSION_SCROLL,
            Some(serde_json::json!({ "id": info.id.0, "to": "sideways" })),
        )
        .unwrap_err();
    assert!(err.to_string().contains("sideways"), "{err}");

    // Text by absolute row, independent of where the viewport is.
    let t = c
        .call(
            method::SESSION_TEXT,
            Some(serde_json::json!({ "id": info.id.0, "from": 0, "to": 2 })),
        )
        .unwrap();
    assert_eq!(t["text"], "line1\nline2\nline3");
    let t = c
        .call(
            method::SESSION_TEXT,
            Some(serde_json::json!({ "id": info.id.0, "from": 39, "to": 39 })),
        )
        .unwrap();
    assert_eq!(t["text"], "line40");

    // Typing while scrolled up snaps the viewport back to the live end.
    let bytes = base64::engine::general_purpose::STANDARD.encode("\n");
    c.call(
        method::SESSION_INPUT,
        Some(serde_json::json!({ "id": info.id.0, "bytes": bytes })),
    )
    .unwrap();
    let after: OutputDelta =
        serde_json::from_value(c.call(method::SESSION_ATTACH, Some(id.clone())).unwrap()).unwrap();
    // The tty echoes the newline, so the buffer may have grown by a row;
    // what matters is that the viewport is back at the live end.
    assert_eq!(
        after.top + u64::from(after.rows),
        after.total,
        "typing follows output: top {} total {}",
        after.top,
        after.total
    );
}
