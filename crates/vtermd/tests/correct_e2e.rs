//! Corrections end to end: a typo typed into a real zsh fails, and
//! `correct.suggest` names the command the user meant.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use base64::Engine as _;
use vt_ipc::Client;
use vt_proto::session::method;

struct Daemon {
    child: Child,
    socket: PathBuf,
    dir: PathBuf,
}

impl Daemon {
    fn start(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("vtermd-correct-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("vtermd.sock");
        let child = Command::new(env!("CARGO_BIN_EXE_vtermd"))
            .arg("--socket")
            .arg(&socket)
            .env("VAMBIANT_TERM_STATE", dir.join("state"))
            .env("VAMBIANT_TERM_CONFIG", dir.join("config"))
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
            .arg(format!("{}/", self.dir.display())) // never a prefix of another test's dir
            .status();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn which(name: &str) -> Option<String> {
    std::env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .map(|d| std::path::Path::new(d).join(name))
        .find(|p| p.is_file())
        .map(|p| p.display().to_string())
}

#[test]
fn a_typo_in_zsh_gets_the_command_the_user_meant() {
    let (Some(zsh), Some(_git)) = (which("zsh"), which("git")) else {
        eprintln!("skipping: zsh and git are needed on PATH");
        return;
    };
    let daemon = Daemon::start("typo");
    let mut c = daemon.client();
    let req = vt_proto::session::NewSession {
        name: Some("typo".into()),
        argv: vec![zsh, "-i".into()],
        cwd: Some(std::env::temp_dir()),
        size: Some((100, 24)),
        ..Default::default()
    };
    let info: vt_proto::session::SessionInfo = serde_json::from_value(
        c.call(method::SESSION_NEW, serde_json::to_value(req).ok())
            .unwrap(),
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(1200));

    let bytes = base64::engine::general_purpose::STANDARD.encode("gti status\n");
    c.call(
        method::SESSION_INPUT,
        Some(serde_json::json!({ "id": info.id.0, "bytes": bytes })),
    )
    .unwrap();
    let start = Instant::now();
    let reply = loop {
        assert!(start.elapsed() < Duration::from_secs(15), "no correction");
        let v = c
            .call(
                method::CORRECT_SUGGEST,
                Some(serde_json::json!({ "session": info.id.0 })),
            )
            .unwrap();
        if v["corrections"].as_array().is_some_and(|a| !a.is_empty()) {
            break v;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(reply["cmdline"], "gti status", "{reply}");
    assert_eq!(reply["exit"], 127);
    assert_eq!(reply["corrections"][0]["command"], "git status", "{reply}");
    assert_eq!(reply["corrections"][0]["rule"], "misspelled-executable");
}
