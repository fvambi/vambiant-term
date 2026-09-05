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
        Self::start_with_env(tag, &[])
    }

    fn dir_for(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("vtermd-e2e-{}-{tag}", std::process::id()))
    }

    fn start_with_env(tag: &str, env: &[(&str, String)]) -> Self {
        let dir = Self::dir_for(tag);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("vtermd.sock");
        let child = Command::new(env!("CARGO_BIN_EXE_vtermd"))
            .arg("--socket")
            .arg(&socket)
            .env("VAMBIANT_TERM_STATE", dir.join("state"))
            .envs(env.iter().map(|(k, v)| (*k, v.as_str())))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(
                std::fs::File::create(dir.join("vtermd.stderr")).map_or(Stdio::null(), Stdio::from),
            )
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

    eprintln!("[e2e] status");
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

    eprintln!("[e2e] wait READY");
    wait_for_text(&mut c, &info.id.0, "READY");
    eprintln!("[e2e] attach");

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

    eprintln!("[e2e] input");
    // Input by name, observed via logs.
    let b64 = base64::engine::general_purpose::STANDARD.encode(b"hello world\n");
    c.call(
        method::SESSION_INPUT,
        Some(serde_json::json!({ "id": "e2e", "bytes": b64 })),
    )
    .unwrap();
    let text = wait_for_text(&mut c, &info.id.0, "GOT:hello world");
    assert!(text.contains("READY"));

    eprintln!("[e2e] resize");
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

    eprintln!("[e2e] watcher+kill");
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
    eprintln!("[e2e] exited seen; list");
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
    // Ended sessions keep their final grid: logs still answer, marked not live.
    let v = c
        .call(
            method::SESSION_LOGS,
            Some(serde_json::json!({ "id": "e2e" })),
        )
        .unwrap();
    assert_eq!(v["live"], false);
    assert!(
        v["text"].as_str().unwrap().contains("GOT:hello world"),
        "{v}"
    );
}

fn spawn_daemon(socket: &std::path::Path, state: &std::path::Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_vtermd"))
        .arg("--socket")
        .arg(socket)
        .env("VAMBIANT_TERM_STATE", state)
        .env("VTERMD_HOLD", env!("CARGO_BIN_EXE_vtermd-hold"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(
            std::fs::File::create(state.parent().unwrap().join("vtermd.stderr"))
                .map_or(Stdio::null(), Stdio::from),
        )
        .spawn()
        .expect("spawn vtermd")
}

fn wait_socket(socket: &std::path::Path) {
    let start = Instant::now();
    while !socket.exists() {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "socket never appeared"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(80));
}

#[test]
fn sessions_survive_a_daemon_restart() {
    let dir = std::env::temp_dir().join(format!("vtermd-e2e-{}-restart", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let socket = dir.join("vtermd.sock");
    let state = dir.join("state");
    std::fs::create_dir_all(&state).unwrap();

    let mut first = spawn_daemon(&socket, &state);
    wait_socket(&socket);
    let mut c = Client::connect(&socket).unwrap();
    let req = NewSession {
        name: Some("survivor".into()),
        argv: vec![
            "/bin/sh".into(),
            "-c".into(),
            "echo BEFORE; read x; echo AFTER:$x; sleep 30".into(),
        ],
        cwd: Some(std::env::temp_dir()),
        ..Default::default()
    };
    let info: SessionInfo = serde_json::from_value(
        c.call(method::SESSION_NEW, serde_json::to_value(req).ok())
            .unwrap(),
    )
    .unwrap();
    wait_for_text(&mut c, &info.id.0, "BEFORE");
    let pid_before = info.pid.expect("pid");

    // Daemon dies hard; the holder keeps the PTY.
    first.kill().unwrap();
    first.wait().unwrap();
    drop(c);
    let _ = std::fs::remove_file(&socket);

    let mut second = spawn_daemon(&socket, &state);
    wait_socket(&socket);
    let mut c = Client::connect(&socket).unwrap();
    let list: Vec<SessionInfo> =
        serde_json::from_value(c.call(method::SESSION_LIST, None).unwrap()).unwrap();
    let s = list
        .iter()
        .find(|s| s.id == info.id)
        .expect("session still listed");
    assert!(
        !s.orphaned,
        "session must be re-adopted, not orphaned: {s:?}"
    );
    assert!(s.readopted, "re-adopted sessions are labelled as such");
    assert_eq!(s.pid, Some(pid_before), "same child process");
    // The grid was rebuilt from the holder's buffer.
    let text = wait_for_text(&mut c, &info.id.0, "BEFORE");
    assert!(text.contains("BEFORE"));
    // And the session is fully alive: input still works.
    let b64 = base64::engine::general_purpose::STANDARD.encode(b"still here\n");
    c.call(
        method::SESSION_INPUT,
        Some(serde_json::json!({ "id": "survivor", "bytes": b64 })),
    )
    .unwrap();
    wait_for_text(&mut c, &info.id.0, "AFTER:still here");

    // Kill through the new daemon: exit is reported via the holder.
    let mut watcher = Client::connect(&socket).unwrap();
    watcher
        .call(
            method::SESSION_ATTACH,
            Some(serde_json::json!({ "id": "survivor" })),
        )
        .unwrap();
    c.call(
        method::SESSION_KILL,
        Some(serde_json::json!({ "id": "survivor", "signal": 9 })),
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
            "no exit notification after re-adoption"
        );
    }
    second.kill().unwrap();
    second.wait().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unreachable_holder_means_orphaned_and_recorded_exit_means_stopped() {
    let dir = std::env::temp_dir().join(format!("vtermd-e2e-{}-orphan", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let socket = dir.join("vtermd.sock");
    let state = dir.join("state");
    std::fs::create_dir_all(&state).unwrap();

    let mut first = spawn_daemon(&socket, &state);
    wait_socket(&socket);
    let mut c = Client::connect(&socket).unwrap();
    let mk = |name: &str, cmd: &str| NewSession {
        name: Some(name.into()),
        argv: vec!["/bin/sh".into(), "-c".into(), cmd.into()],
        cwd: Some(std::env::temp_dir()),
        ..Default::default()
    };
    let orphan: SessionInfo = serde_json::from_value(
        c.call(
            method::SESSION_NEW,
            serde_json::to_value(mk("orphan", "sleep 30")).ok(),
        )
        .unwrap(),
    )
    .unwrap();
    let quick: SessionInfo = serde_json::from_value(
        c.call(
            method::SESSION_NEW,
            serde_json::to_value(mk("quick", "sleep 0.3; exit 7")).ok(),
        )
        .unwrap(),
    )
    .unwrap();
    first.kill().unwrap();
    first.wait().unwrap();
    drop(c);
    let _ = std::fs::remove_file(&socket);
    // Make the orphan's holder unreachable (as after a reboot) and let the
    // quick one exit with nobody attached so its holder records the code.
    let orphan_sock = dir.join(format!("hold-{}.sock", orphan.id.0));
    let _ = std::fs::remove_file(&orphan_sock);
    std::thread::sleep(Duration::from_millis(800));

    let mut second = spawn_daemon(&socket, &state);
    wait_socket(&socket);
    let mut c = Client::connect(&socket).unwrap();
    let list: Vec<SessionInfo> =
        serde_json::from_value(c.call(method::SESSION_LIST, None).unwrap()).unwrap();
    let o = list
        .iter()
        .find(|s| s.id == orphan.id)
        .expect("orphan listed");
    assert!(
        o.orphaned,
        "unreachable holder ⇒ orphaned, never dropped: {o:?}"
    );
    let q = list
        .iter()
        .find(|s| s.id == quick.id)
        .expect("quick listed");
    assert!(
        q.pid.is_none() && !q.orphaned,
        "recorded exit ⇒ stopped: {q:?}"
    );
    // Clean up the orphan's child (its holder is still alive under sleep 30).
    // SAFETY-free: use the pid the first daemon reported.
    if let Some(pid) = orphan.pid {
        let _ = Command::new("kill")
            .arg("-9")
            .arg(format!("-{pid}"))
            .status();
    }
    second.kill().unwrap();
    second.wait().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

/// A fake Claude: a shell that POSTs a PermissionRequest to the daemon's hook
/// receiver exactly like the `http` hook handler would, prints the reply, and
/// then POSTs a Stop.
#[test]
#[allow(clippy::too_many_lines)]
fn claude_permission_request_flows_through_the_inbox() {
    let daemon = Daemon::start("inbox");
    let mut c = daemon.client();
    let script = r#"
        payload='{"hook_event_name":"SessionStart","session_id":"fake-1","cwd":"/tmp","source":"startup","transcript_path":"/tmp/t"}'
        curl -s -X POST -H 'Content-Type: application/json' --data "$payload" "$VAMBIANT_TERM_HOOK_URL" >/dev/null
        perm='{"hook_event_name":"PermissionRequest","session_id":"fake-1","cwd":"/tmp","tool_name":"Bash","tool_use_id":"toolu_9","tool_input":{"command":"rm -rf build"},"permission_mode":"default","prompt_id":"p1","transcript_path":"/tmp/t","permission_suggestions":[]}'
        echo REQUESTING
        reply=$(curl -s -X POST -H 'Content-Type: application/json' --data "$perm" "$VAMBIANT_TERM_HOOK_URL")
        echo "REPLY:$reply"
        stop='{"hook_event_name":"Stop","session_id":"fake-1","cwd":"/tmp","last_assistant_message":"all done","stop_hook_active":false,"transcript_path":"/tmp/t"}'
        curl -s -X POST -H 'Content-Type: application/json' --data "$stop" "$VAMBIANT_TERM_HOOK_URL" >/dev/null
        sleep 30
    "#;
    let req = NewSession {
        name: Some("fake-claude".into()),
        argv: vec!["/bin/sh".into(), "-c".into(), script.into()],
        cwd: Some(std::env::temp_dir()),
        env: vec![],
        size: Some((120, 20)),
        agent: Some(vt_proto::agent::AgentKind::Claude),
    };
    let info: SessionInfo = serde_json::from_value(
        c.call(method::SESSION_NEW, serde_json::to_value(req).ok())
            .unwrap(),
    )
    .unwrap();
    assert!(
        info.capabilities.permission_control,
        "claude sessions advertise control: {info:?}"
    );
    // Provisioning prepended --settings; the settings file exists.
    let list: Vec<SessionInfo> =
        serde_json::from_value(c.call(method::SESSION_LIST, None).unwrap()).unwrap();
    assert_eq!(list[0].agent, vt_proto::agent::AgentKind::Claude);

    wait_for_text(&mut c, &info.id.0, "REQUESTING");
    // The request shows up in the inbox and the session is blocked.
    let start = Instant::now();
    let item = loop {
        let v = c.call("inbox.list", None).unwrap();
        if let Some(item) = v.as_array().and_then(|a| a.first()).cloned() {
            break item;
        }
        assert!(start.elapsed() < Duration::from_secs(10), "no inbox item");
        std::thread::sleep(Duration::from_millis(30));
    };
    assert_eq!(item["request"]["tool"], "Bash");
    assert_eq!(item["session_name"], "fake-claude");
    let got: SessionInfo = serde_json::from_value(
        c.call(
            method::SESSION_GET,
            Some(serde_json::json!({ "id": "fake-claude" })),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(got.state, vt_proto::agent::AgentState::AwaitingInput);

    // Deny from the inbox: the held hook returns the vendor-shaped deny.
    let id = item["id"].as_str().unwrap().to_string();
    c.call(
        "inbox.decide",
        Some(serde_json::json!({ "id": id, "decision": { "behavior": "deny", "reason": "not today" } })),
    )
    .unwrap();
    let text = wait_for_text(&mut c, &info.id.0, "REPLY:");
    assert!(
        text.contains(r#""permissionDecision":"deny""#) || text.contains(r#""behavior":"deny""#),
        "grid: {text}"
    );
    assert!(text.contains("not today"), "reason forwarded: {text}");
    assert!(
        c.call("inbox.list", None)
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );

    // The event log has the whole story.
    let events = c
        .call(
            "agent.events",
            Some(serde_json::json!({ "id": "fake-claude", "after": 0 })),
        )
        .unwrap();
    let kinds: Vec<String> = events
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| {
            e.get(1)
                .and_then(|v| v.get("type"))
                .and_then(|t| t.as_str())
                .map(str::to_owned)
        })
        .collect();
    assert!(kinds.contains(&"session_started".to_string()), "{kinds:?}");
    assert!(kinds.contains(&"approval_needed".to_string()), "{kinds:?}");
    assert!(
        kinds.contains(&"approval_resolved".to_string()),
        "{kinds:?}"
    );
    let start = Instant::now();
    loop {
        let events = c
            .call(
                "agent.events",
                Some(serde_json::json!({ "id": "fake-claude", "after": 0 })),
            )
            .unwrap();
        if events.to_string().contains("all done") {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "Stop never logged"
        );
        std::thread::sleep(Duration::from_millis(30));
    }
    c.call(
        method::SESSION_KILL,
        Some(serde_json::json!({ "id": "fake-claude", "signal": 9 })),
    )
    .unwrap();
}

/// A stand-in for `codex app-server`: one WebSocket client (the daemon's
/// observer), scripted like the real server behaved on 2026-09-05.
fn fake_app_server(
    socket: &std::path::Path,
    to_test: std::sync::mpsc::Sender<serde_json::Value>,
    from_test: std::sync::mpsc::Receiver<&'static str>,
) {
    use vt_agent::ws::WebSocket;
    let listener = std::os::unix::net::UnixListener::bind(socket).unwrap();
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut ws = WebSocket::accept(stream).unwrap();
        let w = ws.writer();
        let send = |v: serde_json::Value| w.lock().unwrap().send_text(&v.to_string()).unwrap();
        let mut resume_attempts = 0;
        while let Ok(Some(text)) = ws.recv_text() {
            let m: serde_json::Value = serde_json::from_str(&text).unwrap();
            let id = m.get("id").cloned();
            match m.get("method").and_then(|x| x.as_str()) {
                Some("initialize") => {
                    send(serde_json::json!({ "id": id, "result": { "userAgent": "fake" } }));
                }
                Some("initialized") => {}
                Some("thread/loaded/list") => {
                    send(
                        serde_json::json!({ "id": id, "result": { "data": ["t1"], "nextCursor": null } }),
                    );
                }
                Some("thread/resume") => {
                    resume_attempts += 1;
                    if resume_attempts == 1 {
                        // The real server answers this until the rollout exists.
                        send(
                            serde_json::json!({ "id": id, "error": { "code": -32600, "message": "no rollout found for thread id t1" } }),
                        );
                        continue;
                    }
                    send(
                        serde_json::json!({ "id": id, "result": { "thread": { "id": "t1", "cwd": "/tmp", "model": "gpt-test" } } }),
                    );
                    send(
                        serde_json::json!({ "method": "thread/started", "params": { "thread": { "id": "t1", "cwd": "/tmp", "model": "gpt-test" } } }),
                    );
                    send(
                        serde_json::json!({ "method": "turn/started", "params": { "threadId": "t1", "turn": { "id": "u1" } } }),
                    );
                    send(
                        serde_json::json!({ "method": "thread/status/changed", "params": { "threadId": "t1", "status": { "type": "active", "activeFlags": ["waitingOnApproval"] } } }),
                    );
                    send(
                        serde_json::json!({ "method": "item/commandExecution/requestApproval", "id": 0, "params": {
                        "threadId": "t1", "turnId": "u1", "itemId": "exec-1", "reason": "outside workspace",
                        "command": "rm -rf build", "cwd": "/tmp", "commandActions": [] } }),
                    );
                }
                Some(other) => panic!("fake app-server got {other}"),
                None => {
                    // A response to our approval request.
                    to_test.send(m.clone()).unwrap();
                    send(
                        serde_json::json!({ "method": "serverRequest/resolved", "params": { "threadId": "t1", "requestId": 0 } }),
                    );
                    send(
                        serde_json::json!({ "method": "item/completed", "params": { "threadId": "t1", "item": { "id": "exec-1", "type": "commandExecution", "command": "rm -rf build", "exitCode": 0, "aggregatedOutput": "" } } }),
                    );
                    send(
                        serde_json::json!({ "method": "thread/tokenUsage/updated", "params": { "threadId": "t1", "tokenUsage": { "total": { "totalTokens": 1000, "inputTokens": 900, "cachedInputTokens": 100, "cacheWriteInputTokens": 0, "outputTokens": 100 }, "modelContextWindow": 10000 } } }),
                    );
                    send(
                        serde_json::json!({ "method": "turn/completed", "params": { "threadId": "t1", "turn": { "id": "u1" } } }),
                    );
                    send(
                        serde_json::json!({ "method": "thread/status/changed", "params": { "threadId": "t1", "status": { "type": "idle" } } }),
                    );
                    // Second approval, which the user answers in the TUI instead.
                    let _ = from_test.recv();
                    send(
                        serde_json::json!({ "method": "item/commandExecution/requestApproval", "id": 1, "params": {
                        "threadId": "t1", "turnId": "u2", "itemId": "exec-2", "command": "git push", "cwd": "/tmp", "commandActions": [] } }),
                    );
                    let _ = from_test.recv();
                    send(
                        serde_json::json!({ "method": "serverRequest/resolved", "params": { "threadId": "t1", "requestId": 1 } }),
                    );
                }
            }
        }
    });
}

fn inbox_first(c: &mut Client, wait: Duration) -> Option<serde_json::Value> {
    let start = Instant::now();
    loop {
        let v = c.call("inbox.list", None).unwrap();
        if let Some(item) = v.as_array().and_then(|a| a.first()).cloned() {
            return Some(item);
        }
        if start.elapsed() > wait {
            return None;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}

fn session_state(c: &mut Client, key: &str) -> vt_proto::agent::AgentState {
    let got: SessionInfo = serde_json::from_value(
        c.call(method::SESSION_GET, Some(serde_json::json!({ "id": key })))
            .unwrap(),
    )
    .unwrap();
    got.state
}

#[test]
#[allow(clippy::too_many_lines)]
fn codex_approval_is_observed_answered_and_withdrawn_through_app_server() {
    // The daemon only connects when a Codex session is created, so the fake
    // can bind after the daemon (which resets its directory) is up.
    let app_socket = Daemon::dir_for("codex").join("fake-codex.sock");
    let daemon = Daemon::start_with_env(
        "codex",
        &[("VTERMD_CODEX_SOCKET", app_socket.display().to_string())],
    );
    let (to_test, from_fake) = std::sync::mpsc::channel();
    let (to_fake, from_test) = std::sync::mpsc::channel();
    fake_app_server(&app_socket, to_test, from_test);
    let mut c = daemon.client();
    let req = NewSession {
        name: Some("fake-codex".into()),
        argv: vec![
            "/bin/sh".into(),
            "-c".into(),
            "echo SOCK=$VAMBIANT_TERM_CODEX_SOCKET; sleep 30".into(),
        ],
        cwd: Some(std::env::temp_dir()),
        env: vec![],
        size: Some((120, 20)),
        agent: Some(vt_proto::agent::AgentKind::Codex),
    };
    let info: SessionInfo = serde_json::from_value(
        c.call(method::SESSION_NEW, serde_json::to_value(req).ok())
            .unwrap(),
    )
    .unwrap();
    assert!(info.capabilities.permission_control && info.capabilities.structured_events);
    let text = wait_for_text(&mut c, &info.id.0, "SOCK=");
    assert!(text.contains("fake-codex.sock"), "socket exported: {text}");

    // The observer resumed the thread (after one refused attempt) and the
    // approval landed in the inbox.
    let item = inbox_first(&mut c, Duration::from_secs(10)).expect("inbox item");
    assert_eq!(item["request"]["tool"], "shell");
    assert_eq!(item["request"]["input"]["command"], "rm -rf build");
    assert_eq!(item["hook_event"], "item/commandExecution/requestApproval");
    assert_eq!(item["session_name"], "fake-codex");
    assert_eq!(
        session_state(&mut c, "fake-codex"),
        vt_proto::agent::AgentState::AwaitingInput
    );

    // Allow from the inbox: the answer reaches the app-server as a JSON-RPC
    // response to request 0 with the vendor's decision shape.
    let id = item["id"].as_str().unwrap().to_string();
    c.call(
        "inbox.decide",
        Some(serde_json::json!({ "id": id, "decision": { "behavior": "allow" } })),
    )
    .unwrap();
    let answer = from_fake
        .recv_timeout(Duration::from_secs(5))
        .expect("answer");
    assert_eq!(answer["id"], 0);
    assert_eq!(answer["result"]["decision"], "accept");
    assert!(inbox_first(&mut c, Duration::from_millis(200)).is_none());
    let start = Instant::now();
    while session_state(&mut c, "fake-codex") != vt_proto::agent::AgentState::Idle {
        assert!(start.elapsed() < Duration::from_secs(5), "turn end → idle");
        std::thread::sleep(Duration::from_millis(30));
    }

    // Second approval answered in the TUI: the inbox item disappears
    // without anyone deciding here.
    to_fake.send("raise").unwrap();
    let second = inbox_first(&mut c, Duration::from_secs(5)).expect("second item");
    assert_eq!(second["request"]["input"]["command"], "git push");
    to_fake.send("resolve").unwrap();
    let start = Instant::now();
    while inbox_first(&mut c, Duration::from_millis(50)).is_some() {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "withdrawn item lingers"
        );
    }

    let events = c
        .call(
            "agent.events",
            Some(serde_json::json!({ "id": "fake-codex", "after": 0 })),
        )
        .unwrap();
    let kinds: Vec<String> = events
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| {
            e.get(1)
                .and_then(|v| v.get("type"))
                .and_then(|t| t.as_str())
                .map(str::to_owned)
        })
        .collect();
    for k in [
        "session_started",
        "approval_needed",
        "approval_resolved",
        "tool_call_end",
        "usage",
    ] {
        assert!(kinds.contains(&k.to_string()), "{k} missing in {kinds:?}");
    }
    assert!(
        events.to_string().contains(r#""by":"agent""#),
        "withdrawal attributed to the agent's prompt: {events}"
    );
    c.call(
        method::SESSION_KILL,
        Some(serde_json::json!({ "id": "fake-codex", "signal": 9 })),
    )
    .unwrap();
}

#[test]
fn reminders_edit_then_allow_and_degraded_labels() {
    let daemon = Daemon::start_with_env(
        "watchdog",
        &[
            ("VTERMD_REMINDER_SECS", "1".into()),
            ("VTERMD_HOOK_GRACE_SECS", "1".into()),
        ],
    );
    let mut c = daemon.client();
    // A "claude" that never calls home: degraded after the grace period.
    let mute = NewSession {
        name: Some("mute".into()),
        argv: vec!["/bin/sh".into(), "-c".into(), "sleep 30".into()],
        cwd: Some(std::env::temp_dir()),
        env: vec![],
        size: Some((80, 24)),
        agent: Some(vt_proto::agent::AgentKind::Claude),
    };
    c.call(method::SESSION_NEW, serde_json::to_value(mute).ok())
        .unwrap();
    // A "claude" that asks for permission and echoes the reply.
    let script = r#"
        perm='{"hook_event_name":"PermissionRequest","session_id":"fake-2","cwd":"/tmp","tool_name":"Bash","tool_use_id":"toolu_1","tool_input":{"command":"rm -rf build"},"permission_mode":"default","prompt_id":"p1","transcript_path":"/tmp/t","permission_suggestions":[]}'
        reply=$(curl -s -X POST -H 'Content-Type: application/json' --data "$perm" "$VAMBIANT_TERM_HOOK_URL")
        echo "REPLY:$reply"
        sleep 30
    "#;
    let asks = NewSession {
        name: Some("asks".into()),
        argv: vec!["/bin/sh".into(), "-c".into(), script.into()],
        cwd: Some(std::env::temp_dir()),
        env: vec![],
        size: Some((160, 24)),
        agent: Some(vt_proto::agent::AgentKind::Claude),
    };
    let info: SessionInfo = serde_json::from_value(
        c.call(method::SESSION_NEW, serde_json::to_value(asks).ok())
            .unwrap(),
    )
    .unwrap();
    let item = inbox_first(&mut c, Duration::from_secs(10)).expect("inbox item");
    let id = item["id"].as_str().unwrap().to_string();

    // Reminders accrue while nobody answers.
    let start = Instant::now();
    loop {
        let item = inbox_first(&mut c, Duration::from_millis(100)).expect("still pending");
        if item["reminders"].as_u64().unwrap_or(0) >= 1 {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "no reminder: {item}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }

    // Degraded label on the mute session, none on the one that called home.
    let start = Instant::now();
    loop {
        let list: Vec<SessionInfo> =
            serde_json::from_value(c.call(method::SESSION_LIST, None).unwrap()).unwrap();
        let mute = list.iter().find(|s| s.name == "mute").unwrap();
        let asks = list.iter().find(|s| s.name == "asks").unwrap();
        assert!(
            asks.degraded.is_none(),
            "live session mislabelled: {asks:?}"
        );
        if let Some(why) = &mute.degraded {
            assert!(why.contains("no hook events"), "{why}");
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "mute never degraded"
        );
        std::thread::sleep(Duration::from_millis(100));
    }

    // Edit-then-allow: the hook reply carries updatedInput.
    c.call(
        "inbox.decide",
        Some(serde_json::json!({ "id": id, "decision": { "behavior": "allow", "updated_input": { "command": "rm -rf build/tmp" } } })),
    )
    .unwrap();
    let text = wait_for_text(&mut c, &info.id.0, "REPLY:");
    assert!(
        text.contains(r#""updatedInput":{"command":"rm -rf build/tmp"}"#),
        "grid: {text}"
    );
    for name in ["mute", "asks"] {
        c.call(
            method::SESSION_KILL,
            Some(serde_json::json!({ "id": name, "signal": 9 })),
        )
        .unwrap();
    }
}
