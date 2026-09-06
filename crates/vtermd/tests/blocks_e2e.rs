//! A shell that emits OSC 133 marks produces command blocks over the
//! daemon: broadcast live on `session.block` and readable afterwards with
//! `session.blocks`.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use vt_ipc::Client;
use vt_proto::session::{NewSession, SessionInfo, method};

struct Daemon {
    child: Child,
    socket: PathBuf,
    dir: PathBuf,
}

impl Daemon {
    fn start(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("vtermd-blocks-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("vtermd.sock");
        let child = Command::new(env!("CARGO_BIN_EXE_vtermd"))
            .arg("--socket")
            .arg(&socket)
            .env("VAMBIANT_TERM_STATE", dir.join("state"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(dir.join("stderr")).map_or(Stdio::null(), Stdio::from))
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

#[test]
fn osc133_marks_become_blocks() {
    let daemon = Daemon::start("133");
    let mut c = daemon.client();

    // A shell that prints a prompt with 133 marks, runs one command that
    // exits 3, then idles so the session stays alive.
    let script = "printf '\\033]133;A\\007$ \\033]133;B\\007\\033]133;C\\007hello\\n\\033]133;D;3\\007'; sleep 30";
    let req = NewSession {
        name: Some("blk".into()),
        argv: vec!["/bin/sh".into(), "-c".into(), script.into()],
        cwd: Some(std::env::temp_dir()),
        size: Some((40, 6)),
        ..Default::default()
    };
    let info: SessionInfo = serde_json::from_value(
        c.call(method::SESSION_NEW, serde_json::to_value(req).ok())
            .unwrap(),
    )
    .unwrap();
    // The marked cycle runs in the session's first output; poll the block
    // query until the command block (C->D, exit 3) has been segmented and
    // stored. This is the deterministic check — the live `session.block`
    // broadcast races a client attaching after the output already flushed.
    let start = Instant::now();
    let block = loop {
        let blocks = c
            .call(
                method::SESSION_BLOCKS,
                Some(serde_json::json!({ "id": info.id.0 })),
            )
            .unwrap();
        let arr = blocks.as_array().cloned().unwrap_or_default();
        if let Some(cmd) = arr
            .into_iter()
            .find(|b| b["block"]["kind"]["kind"] == "command")
        {
            break cmd;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "no command block was segmented"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(block["block"]["kind"]["exit"], 3);
    assert_eq!(block["block"]["confidence"], "marked");
}
