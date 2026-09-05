//! The configuration through the daemon: read, edit with comments kept,
//! reject bad values naming the key, bind keys, and notice external edits.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use vt_ipc::Client;
use vt_proto::session::{method, notification};

struct Daemon {
    child: Child,
    socket: PathBuf,
    dir: PathBuf,
}

impl Daemon {
    fn start(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("vtermd-cfg-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("config")).unwrap();
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

    fn config_path(&self) -> PathBuf {
        self.dir.join("config/config.toml")
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn get_set_and_validate() {
    let daemon = Daemon::start("getset");
    let mut c = daemon.client();

    let v = c.call(method::CONFIG_GET, None).unwrap();
    assert_eq!(v["config"]["font"]["size"], 13.0);
    assert!(v["config_error"].is_null());
    assert!(
        v["fields"].as_array().unwrap().len() > 80,
        "field metadata is served"
    );
    assert!(
        v["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["id"] == "inbox.open")
    );
    assert!(v["themes"]["vambiant-dark"]["background"].is_string());
    assert!(
        v["keymap"]["bindings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|b| b["chord"] == "prefix a")
    );
    assert_eq!(
        v["paths"]["config"],
        daemon.config_path().display().to_string()
    );

    // A comment survives an edit made through the daemon.
    std::fs::write(daemon.config_path(), "# mine\n[font]\nsize = 13.0 # pt\n").unwrap();
    let _ = c.call(method::CONFIG_RELOAD, None).unwrap();
    let new = c
        .call(
            method::CONFIG_SET,
            Some(serde_json::json!({ "key": "font.size", "value": 15 })),
        )
        .unwrap();
    assert_eq!(new["font"]["size"], 15.0);
    let text = std::fs::read_to_string(daemon.config_path()).unwrap();
    assert!(text.starts_with("# mine\n"), "{text}");
    assert!(text.contains("size = 15 # pt"), "{text}");

    let err = c
        .call(
            method::CONFIG_SET,
            Some(serde_json::json!({ "key": "privacy.telemetry", "value": true })),
        )
        .unwrap_err()
        .to_string();
    assert!(err.contains("privacy.telemetry"), "{err}");
    let err = c
        .call(
            method::CONFIG_SET,
            Some(serde_json::json!({ "key": "font.siez", "value": 1 })),
        )
        .unwrap_err()
        .to_string();
    assert!(err.contains("siez"), "{err}");
    let after = c.call(method::CONFIG_GET, None).unwrap();
    assert_eq!(
        after["config"]["privacy"]["telemetry"], false,
        "a rejected edit changes nothing"
    );

    let keymap = c
        .call(
            method::CONFIG_KEYMAP_SET,
            Some(serde_json::json!({ "chord": "cmd+j", "action": "inbox.open" })),
        )
        .unwrap();
    assert!(
        keymap["bindings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|b| b["chord"] == "cmd+j" && b["source"] == "keymap.toml")
    );
    let err = c
        .call(
            method::CONFIG_KEYMAP_SET,
            Some(serde_json::json!({ "chord": "cmd+j", "action": "nope" })),
        )
        .unwrap_err()
        .to_string();
    assert!(err.contains("nope"), "{err}");
    let keymap = c
        .call(
            method::CONFIG_KEYMAP_SET,
            Some(serde_json::json!({ "chord": "cmd+j" })),
        )
        .unwrap();
    assert!(
        !keymap["bindings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|b| b["chord"] == "cmd+j")
    );

    let mut theme: serde_json::Value = after["themes"]["vambiant-dark"].clone();
    theme["name"] = "mine".into();
    let saved = c
        .call(
            method::CONFIG_THEME_SAVE,
            Some(serde_json::json!({ "theme": theme })),
        )
        .unwrap();
    assert!(
        saved["path"]
            .as_str()
            .unwrap()
            .ends_with("themes/mine.toml")
    );
    let after = c.call(method::CONFIG_GET, None).unwrap();
    assert!(after["themes"]["mine"].is_object());
}

#[test]
fn external_edits_are_noticed_and_bad_files_reported() {
    let daemon = Daemon::start("watch");
    let mut c = daemon.client();
    let mut watcher = daemon.client();
    let _ = c.call(method::CONFIG_GET, None).unwrap();
    // The watcher polls once a second; make the mtime move.
    std::thread::sleep(Duration::from_millis(1100));
    std::fs::write(daemon.config_path(), "[cursor]\nstyle = \"blocky\"\n").unwrap();
    let start = Instant::now();
    let changed = loop {
        match watcher.next_notification().unwrap() {
            Some(n) if n.method == notification::CONFIG_CHANGED => break n.params.unwrap(),
            _ => assert!(
                start.elapsed() < Duration::from_secs(5),
                "no config.changed"
            ),
        }
    };
    let err = &changed["config_error"];
    assert!(
        err["file"].as_str().unwrap().ends_with("config.toml"),
        "{changed}"
    );
    assert_eq!(err["line"], 2, "{changed}");
    let v = c.call(method::CONFIG_GET, None).unwrap();
    assert_eq!(
        v["config"]["cursor"]["style"], "block",
        "defaults while the file is broken"
    );
    assert!(
        v["config_error"]["message"]
            .as_str()
            .unwrap()
            .contains("blocky"),
        "{v}"
    );
}
