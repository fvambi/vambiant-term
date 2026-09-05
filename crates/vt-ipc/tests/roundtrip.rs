//! Server/client round trip over a real Unix socket.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::time::Duration;

use vt_ipc::server::ConnId;
use vt_ipc::{Client, Handler, Server};
use vt_proto::jsonrpc::{Request, RpcError};

struct Echo;

impl Handler for Echo {
    fn handle(&self, _conn: ConnId, request: &Request) -> Result<serde_json::Value, RpcError> {
        match request.method.as_str() {
            "echo" => Ok(request.params.clone().unwrap_or(serde_json::Value::Null)),
            "fail" => Err(RpcError::new(
                RpcError::NO_SUCH_SESSION,
                "no session named x",
            )),
            other => Err(RpcError::new(
                RpcError::METHOD_NOT_FOUND,
                format!("unknown method {other}"),
            )),
        }
    }
}

fn temp_socket(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("vt-ipc-test-{}-{name}", std::process::id()));
    dir.join("vtermd.sock")
}

#[test]
fn call_error_and_notification() {
    let path = temp_socket("basic");
    let server = Server::start(&path, Arc::new(Echo)).expect("start");
    let mut client = Client::connect(&path).expect("connect");

    let v = client
        .call("echo", Some(serde_json::json!({"a": 1})))
        .unwrap();
    assert_eq!(v, serde_json::json!({"a": 1}));

    match client.call("fail", None) {
        Err(vt_ipc::IpcError::Remote { code, message, .. }) => {
            assert_eq!(code, RpcError::NO_SUCH_SESSION);
            assert!(message.contains("no session named x"));
        }
        other => panic!("expected remote error, got {other:?}"),
    }
    match client.call("nope", None) {
        Err(vt_ipc::IpcError::Remote { code, .. }) => assert_eq!(code, RpcError::METHOD_NOT_FOUND),
        other => panic!("expected method-not-found, got {other:?}"),
    }

    // Notifications reach connected clients.
    std::thread::sleep(Duration::from_millis(50));
    server.broadcast("session.changed", Some(serde_json::json!({"id": "s1"})));
    let n = client.next_notification().unwrap().expect("notification");
    assert_eq!(n.method, "session.changed");

    // Socket file and directory permissions.
    let dir_mode = std::fs::metadata(path.parent().unwrap())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(dir_mode, 0o700);
    drop(server);
    assert!(!path.exists(), "socket removed on drop");
}

#[test]
fn malformed_line_gets_a_parse_error_not_a_hangup() {
    let path = temp_socket("malformed");
    let _server = Server::start(&path, Arc::new(Echo)).expect("start");
    let mut raw = std::os::unix::net::UnixStream::connect(&path).unwrap();
    raw.write_all(b"this is not json\n").unwrap();
    let mut line = String::new();
    BufReader::new(raw.try_clone().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert!(line.contains("-32700"), "got: {line}");
    // Connection still alive afterwards.
    raw.write_all(br#"{"jsonrpc":"2.0","method":"echo","params":7,"id":9}"#)
        .unwrap();
    raw.write_all(b"\n").unwrap();
    let mut line2 = String::new();
    BufReader::new(raw).read_line(&mut line2).unwrap();
    assert!(line2.contains(r#""result":7"#), "got: {line2}");
}

#[test]
fn a_stalled_client_never_blocks_the_others() {
    let path = temp_socket("stalled");
    let server = Server::start(&path, Arc::new(Echo)).expect("start");
    // A raw connection that never reads.
    let _stalled = std::os::unix::net::UnixStream::connect(&path).unwrap();
    let mut live = Client::connect(&path).expect("connect");
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(server.connections(), 2);
    // Flood until the kernel buffer and the outbox of the stalled client are
    // both full and it gets dropped; the live client drains concurrently.
    let blob = "x".repeat(16 * 1024);
    let drain = std::thread::spawn(move || {
        let mut got = 0usize;
        while let Some(n) = live.next_notification().unwrap() {
            if n.method == "flood" {
                got += 1;
            } else if n.method == "done" {
                break;
            }
        }
        (live, got)
    });
    let started = std::time::Instant::now();
    let mut sent = 0usize;
    while server.connections() == 2 {
        server.broadcast(
            "flood",
            Some(serde_json::json!({ "i": sent, "blob": blob })),
        );
        sent += 1;
        assert!(sent < 20_000, "stalled client was never dropped");
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "broadcast blocked on the stalled client"
        );
    }
    server.broadcast("done", None);
    let (mut live, got) = drain.join().unwrap();
    assert_eq!(got, sent, "live client must receive every message");
    assert_eq!(server.connections(), 1);
    let v = live.call("echo", Some(serde_json::json!(1))).unwrap();
    assert_eq!(v, serde_json::json!(1));
}

#[test]
fn publish_reaches_subscribers_only() {
    let path = temp_socket("publish");
    let server = Server::start(&path, Arc::new(Echo)).expect("start");
    let mut a = Client::connect(&path).expect("a");
    let mut b = Client::connect(&path).expect("b");
    std::thread::sleep(Duration::from_millis(50));
    // Connection ids are 1 and 2 in accept order.
    server.subscribe(ConnId(1), "s1");
    server.publish(
        "s1",
        "session.output",
        Some(serde_json::json!({ "id": "s1" })),
    );
    server.broadcast("session.changed", None);
    let n = a.next_notification().unwrap().unwrap();
    assert_eq!(n.method, "session.output");
    let n = b.next_notification().unwrap().unwrap();
    assert_eq!(n.method, "session.changed", "b must not receive s1 output");
}
