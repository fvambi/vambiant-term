//! Server/client round trip over a real Unix socket.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::time::Duration;

use vt_ipc::{Client, Handler, Server};
use vt_proto::jsonrpc::{Request, RpcError};

struct Echo;

impl Handler for Echo {
    fn handle(&self, request: &Request) -> Result<serde_json::Value, RpcError> {
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
