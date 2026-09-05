//! Spawns the real `vtermd` binary on a private socket and drives it through
//! `vt-ipc` exactly as `vterm` does: create, list, read logs, send input,
//! observe output notifications, kill, observe exit.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use base64::Engine as _;
use vt_ipc::Client;
use vt_proto::session::{NewSession, OutputDelta, SessionInfo, method, notification};

struct Daemon {
    child: Child,
    socket: PathBuf,
    _dir: PathBuf,
}

impl Daemon {
    fn start(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("vtermd-e2e-{}-{tag}", std::process::id()));
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
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "vtermd did not create its socket"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        Self {
            child,
            socket,
            _dir: dir,
        }
    }

    fn client(&self) -> Client {
        let start = Instant::now();
        loop {
            match Client::connect(&self.socket) {
                Ok(c) => return c,
                Err(e) => {
                    assert!(
                        start.elapsed() < Duration::from_secs(5),
                        "cannot connect: {e}"
                    );
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn wait_for_text(client: &mut Client, id: &str, needle: &str) -> String {
    let start = Instant::now();
    loop {
        let v = client
            .call(method::SESSION_LOGS, Some(serde_json::json!({ "id": id })))
            .expect("logs");
        let text = v
            .get("text")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();
        if text.contains(needle) {
            return text;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "never saw {needle:?}; grid was:\n{text}"
        );
        std::thread::sleep(Duration::from_millis(30));
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn create_list_input_logs_kill() {
    let daemon = Daemon::start("basic");
    let mut c = daemon.client();

    let status = c.call(method::DAEMON_STATUS, None).unwrap();
    assert_eq!(
        status.get("sessions").and_then(serde_json::Value::as_u64),
        Some(0)
    );

    let req = NewSession {
        name: Some("e2e".into()),
        argv: vec![
            "/bin/sh".into(),
            "-c".into(),
            "echo READY; read line; echo GOT:$line; sleep 30".into(),
        ],
        cwd: Some(std::env::temp_dir()),
        env: vec![],
        size: Some((60, 10)),
        agent: None,
    };
    let info: SessionInfo = serde_json::from_value(
        c.call(method::SESSION_NEW, serde_json::to_value(req).ok())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(info.name, "e2e");
    assert!(info.pid.is_some());

    let list: Vec<SessionInfo> =
        serde_json::from_value(c.call(method::SESSION_LIST, None).unwrap()).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, info.id);

    wait_for_text(&mut c, &info.id.0, "READY");

    // Attach returns a full snapshot of the right size.
    let snap: OutputDelta = serde_json::from_value(
        c.call(
            method::SESSION_ATTACH,
            Some(serde_json::json!({ "id": "e2e" })),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(snap.full);
    assert_eq!((snap.cols, snap.rows), (60, 10));
    assert_eq!(snap.lines.len(), 10);

    // Input by name, observed via logs.
    let b64 = base64::engine::general_purpose::STANDARD.encode(b"hello world\n");
    c.call(
        method::SESSION_INPUT,
        Some(serde_json::json!({ "id": "e2e", "bytes": b64 })),
    )
    .unwrap();
    let text = wait_for_text(&mut c, &info.id.0, "GOT:hello world");
    assert!(text.contains("READY"));

    // Resize is reflected in the next snapshot.
    c.call(
        method::SESSION_RESIZE,
        Some(serde_json::json!({ "id": "e2e", "cols": 100, "rows": 20 })),
    )
    .unwrap();
    let snap: OutputDelta = serde_json::from_value(
        c.call(
            method::SESSION_ATTACH,
            Some(serde_json::json!({ "id": "e2e" })),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!((snap.cols, snap.rows), (100, 20));

    // A second connection sees output notifications and the exit.
    let mut watcher = daemon.client();
    watcher
        .call(
            method::SESSION_ATTACH,
            Some(serde_json::json!({ "id": "e2e" })),
        )
        .unwrap();
    c.call(
        method::SESSION_KILL,
        Some(serde_json::json!({ "id": "e2e", "signal": 9 })),
    )
    .unwrap();
    let start = Instant::now();
    loop {
        let n = watcher.next_notification().unwrap().expect("notification");
        if n.method == notification::SESSION_EXITED {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "no exit notification"
        );
    }
    // Ended sessions stay listed (from the store) as stopped, never dropped.
    let start = Instant::now();
    loop {
        let list: Vec<SessionInfo> =
            serde_json::from_value(c.call(method::SESSION_LIST, None).unwrap()).unwrap();
        if list.iter().any(|s| s.id == info.id && s.pid.is_none()) {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "session vanished or never stopped: {list:?}"
        );
        std::thread::sleep(Duration::from_millis(30));
    }
    match c.call(
        method::SESSION_LOGS,
        Some(serde_json::json!({ "id": "e2e" })),
    ) {
        Err(vt_ipc::IpcError::Remote { code, .. }) => {
            assert_eq!(code, vt_proto::jsonrpc::RpcError::NO_SUCH_SESSION);
        }
        other => panic!("expected no-such-session after exit, got {other:?}"),
    }
}

#[test]
fn restart_marks_previous_sessions_orphaned() {
    let dir = std::env::temp_dir().join(format!("vtermd-e2e-{}-restart", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let socket = dir.join("vtermd.sock");
    let state = dir.join("state");
    let spawn = || {
        Command::new(env!("CARGO_BIN_EXE_vtermd"))
            .arg("--socket")
            .arg(&socket)
            .env("VAMBIANT_TERM_STATE", &state)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn vtermd")
    };
    let wait_socket = || {
        let start = Instant::now();
        while !socket.exists() {
            assert!(start.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(20));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let mut first = spawn();
    wait_socket();
    let mut c = Client::connect(&socket).unwrap();
    let req = NewSession {
        name: Some("survivor".into()),
        argv: vec!["/bin/sh".into(), "-c".into(), "sleep 30".into()],
        ..Default::default()
    };
    let info: SessionInfo = serde_json::from_value(
        c.call(method::SESSION_NEW, serde_json::to_value(req).ok())
            .unwrap(),
    )
    .unwrap();
    first.kill().unwrap();
    first.wait().unwrap();
    drop(c);
    let _ = std::fs::remove_file(&socket);

    let mut second = spawn();
    wait_socket();
    let mut c = Client::connect(&socket).unwrap();
    let list: Vec<SessionInfo> =
        serde_json::from_value(c.call(method::SESSION_LIST, None).unwrap()).unwrap();
    let s = list
        .iter()
        .find(|s| s.id == info.id)
        .expect("previous session still listed");
    assert!(
        s.orphaned,
        "previous daemon's session must be marked orphaned, not dropped: {s:?}"
    );
    second.kill().unwrap();
    second.wait().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}
