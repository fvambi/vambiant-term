//! Both adapters against a canned server: the request they send and the
//! stream they assemble. The server is a bare TCP listener answering one
//! HTTP request with a fixed SSE body, so nothing here needs a network.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use vt_ai::anthropic::{Anthropic, one_shot};
use vt_ai::http::Http;
use vt_ai::openai::OpenAiChat;
use vt_ai::provider::{Chunk, Content, Provider, ProviderError, SamplingSupport, StopReason};
use vt_ai::route::{Kind, ProvidersFile};

/// Serves `body` (with SSE headers) to the first request and hands back
/// the request that arrived. `status` lets a test exercise error paths.
fn serve(status: u16, body: &'static str) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = stream.read(&mut chunk).unwrap_or(0);
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&buf);
            if let Some(head_end) = text.find("\r\n\r\n") {
                let head = &text[..head_end];
                let len: usize = head
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
            "HTTP/1.1 {status} X\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            if status == 200 {
                "text/event-stream"
            } else {
                "application/json"
            },
            body.len()
        );
        stream.write_all(response.as_bytes()).unwrap();
        stream.flush().unwrap();
    });
    (format!("http://{addr}"), rx)
}

const ANTHROPIC_STREAM: &str = "event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":12,\"cache_read_input_tokens\":3}}}\n\n\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
event: ping\n\
data: {\"type\":\"ping\"}\n\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Run \"}}\n\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"ls -la\"}}\n\n\
event: content_block_stop\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"tu_1\",\"name\":\"run\",\"input\":{}}}\n\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"cmd\\\": \"}}\n\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"ls\\\"}\"}}\n\n\
event: content_block_stop\n\
data: {\"type\":\"content_block_stop\",\"index\":1}\n\n\
event: some_future_event\n\
data: {\"type\":\"some_future_event\"}\n\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":9}}\n\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n\n";

#[test]
fn anthropic_streams_text_and_tool_calls_and_drops_default_only_sampling() {
    let (base, rx) = serve(200, ANTHROPIC_STREAM);
    let p = Anthropic {
        name: "claude-strong".into(),
        base_url: base,
        api_key: Some("sk-ant-test".into()),
        models: vec!["opus".into(), "sonnet".into()],
        default_only_sampling: vec!["opus".into()],
        http: Http::new(Duration::from_secs(5)),
    };
    let mut req = one_shot("opus", Some("be terse"), "list files", 64);
    req.temperature = Some(0.2);
    let mut chunks = Vec::new();
    let done = p.stream(&req, &mut |c| chunks.push(c)).unwrap();

    let sent = rx.recv().unwrap();
    assert!(sent.contains("x-api-key: sk-ant-test"), "{sent}");
    assert!(sent.contains("anthropic-version: 2023-06-01"));
    let body_json: serde_json::Value =
        serde_json::from_str(sent.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(body_json["system"], "be terse");
    assert_eq!(body_json["stream"], true);
    assert!(
        body_json.get("temperature").is_none(),
        "DefaultOnly model: temperature dropped"
    );
    assert_eq!(body_json["messages"][0]["content"][0]["text"], "list files");

    assert!(chunks.contains(&Chunk::TextDelta("Run ".into())));
    assert!(chunks.contains(&Chunk::ToolUseStart {
        id: "tu_1".into(),
        name: "run".into()
    }));
    assert_eq!(
        done.content,
        vec![
            Content::Text {
                text: "Run ls -la".into()
            },
            Content::ToolUse {
                id: "tu_1".into(),
                name: "run".into(),
                input: serde_json::json!({"cmd": "ls"})
            },
        ]
    );
    assert_eq!(done.stop, StopReason::ToolUse);
    assert_eq!(
        (
            done.usage.input_tokens,
            done.usage.output_tokens,
            done.usage.cache_read_tokens
        ),
        (12, 9, 3)
    );

    // A Full-sampling model keeps its temperature.
    let (base2, rx2) = serve(200, ANTHROPIC_STREAM);
    let p2 = Anthropic {
        base_url: base2,
        ..p
    };
    let mut req2 = one_shot("sonnet", None, "x", 8);
    req2.temperature = Some(0.5);
    p2.stream(&req2, &mut |_| {}).unwrap();
    let sent2 = rx2.recv().unwrap();
    let body2: serde_json::Value =
        serde_json::from_str(sent2.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(body2["temperature"], 0.5, "{sent2}");
    assert_eq!(
        p2.caps("sonnet").unwrap().temperature,
        SamplingSupport::Full
    );
    assert_eq!(
        p2.caps("opus").unwrap().temperature,
        SamplingSupport::DefaultOnly
    );
    assert!(p2.caps("nope").is_none());
}

#[test]
fn anthropic_without_a_key_never_connects() {
    let p = Anthropic {
        name: "claude-strong".into(),
        base_url: "http://127.0.0.1:1".into(),
        api_key: None,
        models: vec!["m".into()],
        default_only_sampling: vec![],
        http: Http::new(Duration::from_secs(1)),
    };
    let err = p
        .stream(&one_shot("m", None, "x", 8), &mut |_| {})
        .unwrap_err();
    assert!(matches!(err, ProviderError::MissingKey { .. }));
    assert!(err.to_string().contains("vterm ai key set claude-strong"));
}

#[test]
fn http_errors_carry_status_and_body_but_no_key() {
    let (base, _rx) = serve(
        429,
        "{\"error\":{\"message\":\"rate limited, retry in 7s\"}}",
    );
    let p = Anthropic {
        name: "c".into(),
        base_url: base,
        api_key: Some("sk-ant-SECRET".into()),
        models: vec!["m".into()],
        default_only_sampling: vec![],
        http: Http::new(Duration::from_secs(5)),
    };
    let err = p
        .stream(&one_shot("m", None, "x", 8), &mut |_| {})
        .unwrap_err();
    let text = err.to_string();
    assert!(
        text.contains("429") && text.contains("rate limited"),
        "{text}"
    );
    assert!(!text.contains("SECRET"));
}

const OPENAI_STREAM: &str = "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"content\":\", world\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"run\",\"arguments\":\"\"}}]}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"cmd\\\":\\\"pwd\\\"}\"}}]}}]}\n\n\
data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":4}}\n\n\
data: [DONE]\n\n";

#[test]
fn openai_compatible_streams_and_maps_tool_calls() {
    let (base, rx) = serve(200, OPENAI_STREAM);
    let p = OpenAiChat {
        name: "local".into(),
        base_url: base,
        api_key: None,
        models: vec!["qwen".into()],
        key_required: false,
        http: Http::new(Duration::from_secs(5)),
    };
    let done = p
        .stream(&one_shot("qwen", Some("sys"), "hi", 32), &mut |_| {})
        .unwrap();
    let sent = rx.recv().unwrap();
    assert!(
        !sent.to_ascii_lowercase().contains("authorization:"),
        "no key, no header: {sent}"
    );
    let body: serde_json::Value =
        serde_json::from_str(sent.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["messages"][1]["content"], "hi");
    assert_eq!(
        done.content,
        vec![
            Content::Text {
                text: "Hello, world".into()
            },
            Content::ToolUse {
                id: "call_1".into(),
                name: "run".into(),
                input: serde_json::json!({"cmd": "pwd"})
            },
        ]
    );
    assert_eq!(done.stop, StopReason::ToolUse);
    assert_eq!(done.usage.input_tokens, 5);
    assert_eq!(done.usage.output_tokens, 4);

    let strict = OpenAiChat {
        key_required: true,
        api_key: None,
        base_url: "http://127.0.0.1:1".into(),
        ..p
    };
    assert!(matches!(
        strict.stream(&one_shot("qwen", None, "x", 8), &mut |_| {}),
        Err(ProviderError::MissingKey { .. })
    ));
}

#[test]
fn providers_file_defaults_parse_and_routes_resolve() {
    let f = ProvidersFile::parse(vt_ai::route::DEFAULT_FILE).unwrap();
    assert!(f.profiles.len() >= 3);
    let strong = f.route("ask").expect("ask routes somewhere");
    assert_eq!(strong.kind, Kind::Anthropic);
    assert!(strong.models.contains(&strong.model));
    assert!(
        f.route("classify").is_none(),
        "classify is deterministic: no model"
    );
    assert!(f.pricing(&strong.model).input > 0.0);
    let free = f.pricing("nope").estimate(vt_ai::Usage {
        input_tokens: 1_000_000,
        ..Default::default()
    });
    assert!(free.abs() < 1e-12, "unlisted models cost nothing: {free}");
    let est = f.pricing(&strong.model).estimate(vt_ai::Usage {
        input_tokens: 1_000_000,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
    });
    assert!((est - f.pricing(&strong.model).input).abs() < 1e-9);

    let bad = ProvidersFile::parse(
        "[[profile]]\nname='a'\nkind='ollama'\nmodel='m'\n[routes]\nask='zzz'\n",
    )
    .unwrap_err();
    assert!(bad.contains("unknown profile `zzz`"), "{bad}");
    let missing = ProvidersFile::load(std::path::Path::new("/nonexistent/providers.toml")).unwrap();
    assert_eq!(
        missing.profiles.len(),
        f.profiles.len(),
        "no file means the bundled defaults"
    );
}
