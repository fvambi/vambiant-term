//! A shell that emits OSC 133 marks produces command blocks over the
//! daemon: broadcast live on `session.block` and readable afterwards with
//! `session.blocks`.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use base64::Engine as _;
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

#[test]
fn injected_zsh_integration_produces_blocks() {
    if which_zsh().is_none() {
        eprintln!("skipping: no zsh on PATH");
        return;
    }
    let daemon = Daemon::start("zsh");
    let mut c = daemon.client();

    // A real zsh, no marks in the command itself — the daemon's injected
    // integration must emit them. Run one command, then keep the shell
    // alive by reading (interactive zsh stays up on its own).
    let req = NewSession {
        name: Some("z".into()),
        argv: vec![which_zsh().unwrap(), "-i".into()],
        cwd: Some(std::env::temp_dir()),
        size: Some((80, 24)),
        ..Default::default()
    };
    let info: SessionInfo = serde_json::from_value(
        c.call(method::SESSION_NEW, serde_json::to_value(req).ok())
            .unwrap(),
    )
    .unwrap();

    // Give zsh a moment to print its first prompt (emits A), then run a
    // command (B on the prompt, C on preexec, D on the next precmd).
    std::thread::sleep(Duration::from_millis(800));
    // Also record where zsh keeps its history: macOS /etc/zshrc derives
    // HISTFILE from ZDOTDIR, and our injected ZDOTDIR must not capture it.
    let hist_probe = daemon.dir.join("histfile");
    let cmd = format!(
        "false; print -r -- \"$HISTFILE\" > {}\n",
        hist_probe.display()
    );
    let bytes = base64::engine::general_purpose::STANDARD.encode(cmd);
    c.call(
        method::SESSION_INPUT,
        Some(serde_json::json!({ "id": info.id.0, "bytes": bytes })),
    )
    .unwrap();

    let start = Instant::now();
    loop {
        let blocks = c
            .call(
                method::SESSION_BLOCKS,
                Some(serde_json::json!({ "id": info.id.0 })),
            )
            .unwrap();
        let arr = blocks.as_array().cloned().unwrap_or_default();
        // The line exits 0 (the redirect is last); the injected 633;E must
        // carry the command line verbatim.
        if let Some(b) = arr.iter().find(|b| {
            b["block"]["kind"]["kind"] == "command"
                && b["block"]["kind"]["cmdline"]
                    .as_str()
                    .is_some_and(|c| c.starts_with("false; print"))
        }) {
            assert_eq!(b["block"]["kind"]["exit"], 0, "{b}");
            assert_eq!(b["block"]["confidence"], "marked");
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "injected zsh integration produced no exit-1 command block; blocks: {}",
            serde_json::to_string(&arr).unwrap()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let histfile = std::fs::read_to_string(&hist_probe).unwrap_or_default();
    let ours = daemon.dir.join("state").join("shell-integration");
    assert!(
        !histfile.trim().is_empty() && !histfile.starts_with(&ours.display().to_string()),
        "HISTFILE was diverted into the integration dir: {histfile:?}"
    );
}

fn which_zsh() -> Option<String> {
    for dir in std::env::var("PATH").unwrap_or_default().split(':') {
        let p = std::path::Path::new(dir).join("zsh");
        if p.is_file() {
            return Some(p.display().to_string());
        }
    }
    None
}
