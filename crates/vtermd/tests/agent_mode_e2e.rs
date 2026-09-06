//! Agent Mode's tool loop end to end: a canned OpenAI-compatible model
//! asks to run a command, the request waits in the inbox with its
//! verdict, the decision runs (or refuses) the command in a real zsh, and
//! the block's output goes back to the model as the tool result.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use vt_ipc::Client;
use vt_proto::session::method;
use vt_proto::session::notification::{AI_DONE, AI_TOOL_REQUEST, AI_TOOL_RESULT};

struct Daemon {
    child: Child,
    socket: PathBuf,
    dir: PathBuf,
}

impl Daemon {
    fn start(tag: &str, base_url: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("vtermd-agent-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("config")).unwrap();
        std::fs::write(
            dir.join("config/providers.toml"),
            format!(
                "[[profile]]\nname = \"mock\"\nkind = \"compat\"\nbase_url = \"{base_url}\"\nmodel = \"test-model\"\n\n[routes]\nagent = \"mock\"\n"
            ),
        )
        .unwrap();
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
            thread::sleep(Duration::from_millis(20));
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
            thread::sleep(Duration::from_millis(20));
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

/// Serves `bodies` to successive requests, recording each request.
fn mock_sequence(bodies: Vec<String>) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for body in bodies {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut buf = Vec::new();
            let mut chunk = [0u8; 8192];
            loop {
                let n = stream.read(&mut chunk).unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
                let text = String::from_utf8_lossy(&buf);
                if let Some(head_end) = text.find("\r\n\r\n") {
                    let len: usize = text[..head_end]
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse().unwrap())
                        })
                        .unwrap_or(0);
                    if buf.len() >= head_end + 4 + len {
                        break;
                    }
                }
            }
            let _ = tx.send(String::from_utf8_lossy(&buf).into_owned());
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
        }
    });
    (format!("http://{addr}"), rx)
}

fn tool_call_body(command: &str) -> String {
    let args = serde_json::json!({ "command": command, "why": "to see it" }).to_string();
    let args = serde_json::to_string(&args).unwrap();
    format!(
        "data: {{\"choices\":[{{\"delta\":{{\"content\":\"Let me check. \"}}}}]}}\n\n\
         data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":[{{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{{\"name\":\"run_command\",\"arguments\":\"\"}}}}]}}}}]}}\n\n\
         data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":[{{\"index\":0,\"function\":{{\"arguments\":{args}}}}}]}}}}]}}\n\n\
         data: {{\"choices\":[{{\"delta\":{{}},\"finish_reason\":\"tool_calls\"}}],\"usage\":{{\"prompt_tokens\":50,\"completion_tokens\":12}}}}\n\n\
         data: [DONE]\n\n"
    )
}

fn text_body(text: &str) -> String {
    format!(
        "data: {{\"choices\":[{{\"delta\":{{\"content\":\"{text}\"}}}}]}}\n\n\
         data: {{\"choices\":[{{\"delta\":{{}},\"finish_reason\":\"stop\"}}],\"usage\":{{\"prompt_tokens\":80,\"completion_tokens\":6}}}}\n\n\
         data: [DONE]\n\n"
    )
}

fn which_zsh() -> Option<String> {
    std::env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .map(|d| std::path::Path::new(d).join("zsh"))
        .find(|p| p.is_file())
        .map(|p| p.display().to_string())
}

fn zsh_session(c: &mut Client, name: &str) -> String {
    let req = vt_proto::session::NewSession {
        name: Some(name.into()),
        argv: vec![which_zsh().unwrap(), "-i".into()],
        cwd: Some(std::env::temp_dir()),
        size: Some((100, 24)),
        ..Default::default()
    };
    let info: vt_proto::session::SessionInfo = serde_json::from_value(
        c.call(method::SESSION_NEW, serde_json::to_value(req).ok())
            .unwrap(),
    )
    .unwrap();
    // zsh prints its first prompt (the A mark) before it can take a line.
    thread::sleep(Duration::from_millis(1200));
    info.id.0
}

fn next(watcher: &mut Client, request: &str, methods: &[&str]) -> (String, serde_json::Value) {
    let start = Instant::now();
    loop {
        assert!(start.elapsed() < Duration::from_secs(20), "no {methods:?}");
        let n = watcher.next_notification().unwrap().expect("notification");
        let params = n.params.clone().unwrap_or_default();
        if params["request"] == request && methods.contains(&n.method.as_str()) {
            return (n.method, params);
        }
    }
}

#[test]
fn an_allowed_command_runs_in_the_session_and_its_output_reaches_the_model() {
    if which_zsh().is_none() {
        eprintln!("skipping: no zsh on PATH");
        return;
    }
    let (base, rx) = mock_sequence(vec![
        tool_call_body("echo agent-hi"),
        text_body("Done: it printed agent-hi."),
    ]);
    let daemon = Daemon::start("allow", &base);
    let mut watcher = daemon.client();
    let mut c = daemon.client();
    let session = zsh_session(&mut c, "agent-allow");

    let v = c
        .call(
            method::AI_ASK,
            Some(serde_json::json!({ "prompt": "what does echo print?", "session": session, "agent": true })),
        )
        .unwrap();
    let request = v["request"].as_str().unwrap().to_owned();

    let (_, req) = next(&mut watcher, &request, &[AI_TOOL_REQUEST]);
    assert_eq!(req["command"], "echo agent-hi", "{req}");
    assert_eq!(req["verdict"]["class"], "benign", "{req}");
    assert_eq!(req["decision"], "ask", "autonomy is off: {req}");
    assert_eq!(req["applied"], false);
    let approval = req["approval"].as_str().expect("an inbox id").to_owned();

    // It waits in the same inbox as a vendor hook would.
    let inbox = c.call("inbox.list", None).unwrap();
    let item = inbox
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == approval)
        .cloned()
        .expect("inbox item");
    assert_eq!(item["request"]["source"], "agent-mode");
    assert_eq!(item["request"]["input"]["command"], "echo agent-hi");
    assert_eq!(item["verdict"]["class"], "benign");
    c.call(
        "inbox.decide",
        Some(serde_json::json!({ "id": approval, "decision": { "behavior": "allow", "updated_input": null } })),
    )
    .unwrap();

    let (_, result) = next(&mut watcher, &request, &[AI_TOOL_RESULT]);
    assert_eq!(result["exit"], 0, "{result}");
    assert!(
        result["output"].as_str().unwrap().contains("agent-hi"),
        "{result}"
    );
    let (_, done) = next(&mut watcher, &request, &[AI_DONE]);
    assert!(
        done["text"]
            .as_str()
            .unwrap()
            .contains("Done: it printed agent-hi."),
        "{done}"
    );
    assert_eq!(
        done["usage"]["input_tokens"], 130,
        "summed over both turns: {done}"
    );

    let first = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(first.contains("run_command"), "tools offered: {first}");
    let second = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(
        second.contains("\"role\":\"tool\"") || second.contains("\"role\": \"tool\""),
        "{second}"
    );
    assert!(second.contains("exit 0"), "{second}");
    assert!(second.contains("agent-hi"), "{second}");
}

#[test]
fn a_denied_command_never_runs_and_the_model_hears_why() {
    if which_zsh().is_none() {
        eprintln!("skipping: no zsh on PATH");
        return;
    }
    let (base, rx) = mock_sequence(vec![
        tool_call_body("touch /tmp/agent-denied-marker"),
        text_body("Understood."),
    ]);
    let daemon = Daemon::start("deny", &base);
    let mut watcher = daemon.client();
    let mut c = daemon.client();
    let session = zsh_session(&mut c, "agent-deny");
    let _ = std::fs::remove_file("/tmp/agent-denied-marker");

    let v = c
        .call(
            method::AI_ASK,
            Some(
                serde_json::json!({ "prompt": "make a marker", "session": session, "agent": true }),
            ),
        )
        .unwrap();
    let request = v["request"].as_str().unwrap().to_owned();
    let (_, req) = next(&mut watcher, &request, &[AI_TOOL_REQUEST]);
    let approval = req["approval"].as_str().unwrap().to_owned();
    c.call(
        "inbox.decide",
        Some(serde_json::json!({ "id": approval, "decision": { "behavior": "deny", "reason": "not today" } })),
    )
    .unwrap();
    let (_, result) = next(&mut watcher, &request, &[AI_TOOL_RESULT]);
    assert_eq!(result["denied"], true, "{result}");
    assert_eq!(result["reason"], "not today");
    let (_, done) = next(&mut watcher, &request, &[AI_DONE]);
    assert!(done["text"].as_str().unwrap().contains("Understood."));
    assert!(
        !std::path::Path::new("/tmp/agent-denied-marker").exists(),
        "the command ran"
    );
    let _ = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let second = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(second.contains("denied: not today"), "{second}");
}
