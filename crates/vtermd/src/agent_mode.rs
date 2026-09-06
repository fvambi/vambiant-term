//! Agent Mode's tool loop (ADR-0011 D2, docs/06 §6): the model may ask to
//! run a command through `run_command`; the request is classified, put
//! through the policy engine and, unless a rule decided under autonomy,
//! waits in the inbox like any vendor hook. An allowed command is typed
//! into the session at its prompt, its block's output (redacted, fail
//! closed) goes back as the tool result, and the loop continues until
//! the model stops or the turn cap is reached.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use vt_ai::provider::{Content, Message, Role, Tool, Usage};
use vt_proto::approval::{ApprovalId, ApprovalRequest, Decision};
use vt_proto::jsonrpc::RpcError;
use vt_proto::session::SessionId;
use vt_proto::session::notification::{
    AI_CHUNK, AI_DONE, AI_ERROR, AI_TOOL_REQUEST, AI_TOOL_RESULT,
};
use vt_store::Store;

use crate::agents::{Agents, InboxItem};
use crate::ai::{HistoryTurn, Prepared, finish, internal, prepare, request_id};
use crate::registry::{Registry, SessionHandle, now};
use crate::session::SessionCmd;

/// Tool calls per request before the loop gives up.
const MAX_TURNS: usize = 12;
/// How long one command may run before its output is reported as pending.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(300);

const AGENT_RULES: &str = "You may run shell commands in the user's terminal with the run_command tool. \
    Each command is shown to the user with its safety classification and runs only after they approve it, \
    so prefer one command at a time, explain briefly what you intend, and never chain destructive steps. \
    The tool result carries the exit status and the command's output.";

fn run_command_tool() -> Tool {
    Tool {
        name: "run_command".into(),
        description: "Run a shell command in the user's terminal session after they approve it. \
            Returns the exit status and output."
            .into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "The command line, as typed at the prompt." },
                "why": { "type": "string", "description": "One sentence on what it is for; shown to the user." }
            },
            "required": ["command"]
        }),
    }
}

/// `ai.ask` with `agent = true`: `{ request }` at once; then `ai.chunk`,
/// `ai.tool_request`/`ai.tool_result` per command, and `ai.done` or
/// `ai.error`, all tagged with the request and the session.
pub fn run(
    registry: &Arc<Registry>,
    store: &Arc<Mutex<Store>>,
    prompt: &str,
    feature: &str,
    session: Option<&str>,
    history: &[HistoryTurn],
) -> Result<serde_json::Value, RpcError> {
    let _ = feature;
    let Some(session) = session else {
        return Err(RpcError::new(
            RpcError::INVALID_PARAMS,
            "agent mode needs a session to run commands in",
        ));
    };
    let handle = registry.find(session).ok_or_else(|| {
        RpcError::new(RpcError::INVALID_PARAMS, format!("no session `{session}`"))
    })?;
    let session = handle
        .info
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .id
        .clone();
    let request = request_id();
    let registry = Arc::clone(registry);
    let store = Arc::clone(store);
    let prompt = prompt.to_owned();
    let history = history.to_vec();
    let id = request.clone();
    std::thread::Builder::new()
        .name(format!("ai-agent-{request}"))
        .spawn(move || drive(&registry, &store, &id, &session, &prompt, &history))
        .map_err(|e| internal(format!("cannot start the agent thread: {e}")))?;
    Ok(serde_json::json!({ "request": request }))
}

/// The loop: prepare once, then stream, run every requested command,
/// feed the results back, until the model stops or the cap is hit.
fn drive(
    registry: &Arc<Registry>,
    store: &Arc<Mutex<Store>>,
    id: &str,
    session: &SessionId,
    prompt: &str,
    history: &[HistoryTurn],
) {
    let Some(server) = registry.server() else {
        return;
    };
    let tag = |mut v: serde_json::Value| {
        v["request"] = serde_json::Value::String(id.to_owned());
        v["id"] = serde_json::Value::String(session.0.clone());
        Some(v)
    };
    let mut p = match prepare(registry, store, prompt, "agent", Some(&session.0), history) {
        Ok(p) => p,
        Err(e) => {
            server.broadcast(AI_ERROR, tag(serde_json::json!({ "message": e.message })));
            return;
        }
    };
    p.req.tools = vec![run_command_tool()];
    p.req.system = Some(format!(
        "{}\n\n{AGENT_RULES}",
        p.req.system.take().unwrap_or_default()
    ));
    let mut usage = Usage::default();
    let mut cost = 0.0;
    let mut text_so_far = String::new();
    for _ in 0..MAX_TURNS {
        let outcome = crate::ai::run_stream(&p, &mut |chunk| {
            if let vt_ai::Chunk::TextDelta(delta) = chunk {
                server.broadcast(AI_CHUNK, tag(serde_json::json!({ "delta": delta })));
            }
        });
        let done = match outcome {
            Ok(d) => d,
            Err((e, next)) => {
                // Fall back only before anything was sent on this provider's behalf.
                if p.req.messages.len() <= 1
                    && let Some(again) = fall_back(
                        registry,
                        store,
                        prompt,
                        session,
                        history,
                        next.as_deref(),
                        &p.profile,
                        &e,
                    )
                {
                    p = again;
                    continue;
                }
                server.broadcast(
                    AI_ERROR,
                    tag(serde_json::json!({ "message": e.to_string() })),
                );
                return;
            }
        };
        account(&mut usage, &mut cost, &mut text_so_far, &done, p.pricing);
        let calls: Vec<(String, String, serde_json::Value)> = done
            .content
            .iter()
            .filter_map(|c| match c {
                Content::ToolUse { id, name, input } => {
                    Some((id.clone(), name.clone(), input.clone()))
                }
                _ => None,
            })
            .collect();
        if calls.is_empty() {
            let mut reply = finish(store, &p, &done);
            reply["text"] = serde_json::Value::String(text_so_far);
            reply["usage"] = serde_json::to_value(usage).unwrap_or_default();
            reply["cost_usd_estimate"] = serde_json::json!(cost);
            server.broadcast(AI_DONE, tag(reply));
            return;
        }
        p.req.messages.push(Message {
            role: Role::Assistant,
            content: done.content.clone(),
        });
        let results = dispatch(registry, &server, &tag, session, calls);
        p.req.messages.push(Message {
            role: Role::User,
            content: results,
        });
    }
    server.broadcast(
        AI_ERROR,
        tag(
            serde_json::json!({ "message": format!("agent stopped after {MAX_TURNS} tool calls") }),
        ),
    );
}

/// Every tool call of one turn, in order; unknown tools get an error result.
fn dispatch(
    registry: &Arc<Registry>,
    server: &vt_ipc::Server,
    tag: &dyn Fn(serde_json::Value) -> Option<serde_json::Value>,
    session: &SessionId,
    calls: Vec<(String, String, serde_json::Value)>,
) -> Vec<Content> {
    let handle = handle_of(registry, session);
    calls
        .into_iter()
        .map(|(tool_use, name, input)| {
            if name == "run_command" {
                run_command(
                    registry,
                    server,
                    tag,
                    session,
                    handle.as_ref(),
                    &tool_use,
                    &input,
                )
            } else {
                Content::ToolResult {
                    tool_use_id: tool_use,
                    content: format!("unknown tool `{name}`"),
                    is_error: true,
                }
            }
        })
        .collect()
}

/// The next profile in the chain, prepared for agent mode, or `None`.
#[allow(clippy::too_many_arguments)] // one call site; the pieces are the turn's own facts
fn fall_back(
    registry: &Arc<Registry>,
    store: &Arc<Mutex<Store>>,
    prompt: &str,
    session: &SessionId,
    history: &[HistoryTurn],
    next: Option<&str>,
    failed: &str,
    err: &vt_ai::ProviderError,
) -> Option<Prepared> {
    let next = next?;
    let mut again = crate::ai::prepare_from(
        registry,
        store,
        prompt,
        "agent",
        Some(&session.0),
        history,
        Some(next),
    )
    .ok()?;
    eprintln!("vtermd: agent profile `{failed}` failed ({err}); falling back to `{next}`");
    again.req.tools = vec![run_command_tool()];
    again.req.system = Some(format!(
        "{}\n\n{AGENT_RULES}",
        again.req.system.take().unwrap_or_default()
    ));
    Some(again)
}

/// Totals across turns; the text the user saw streamed, joined.
fn account(
    usage: &mut Usage,
    cost: &mut f64,
    text_so_far: &mut String,
    done: &vt_ai::Completion,
    pricing: vt_ai::cost::Pricing,
) {
    usage.input_tokens += done.usage.input_tokens;
    usage.output_tokens += done.usage.output_tokens;
    usage.cache_read_tokens += done.usage.cache_read_tokens;
    usage.cache_write_tokens += done.usage.cache_write_tokens;
    *cost += pricing.estimate(done.usage);
    for c in &done.content {
        if let Content::Text { text } = c {
            if !text_so_far.is_empty() && !text.is_empty() {
                text_so_far.push_str("\n\n");
            }
            text_so_far.push_str(text);
        }
    }
}

fn handle_of(registry: &Registry, session: &SessionId) -> Option<SessionHandle> {
    registry.find(&session.0)
}

/// One `run_command`: classify, decide (rule or inbox), run at the prompt,
/// report the block's output. Every branch broadcasts what happened.
fn run_command(
    registry: &Arc<Registry>,
    server: &vt_ipc::Server,
    tag: &dyn Fn(serde_json::Value) -> Option<serde_json::Value>,
    session: &SessionId,
    handle: Option<&SessionHandle>,
    tool_use: &str,
    input: &serde_json::Value,
) -> Content {
    let error = |content: String| Content::ToolResult {
        tool_use_id: tool_use.to_owned(),
        content,
        is_error: true,
    };
    let Some(command) = input
        .get("command")
        .and_then(|c| c.as_str())
        .map(str::trim)
        .filter(|c| !c.is_empty())
    else {
        return error("run_command needs a non-empty `command`".into());
    };
    let Some(handle) = handle else {
        return error("the session is gone".into());
    };
    let (cwd, name, generic) = {
        let info = handle.info.lock().unwrap_or_else(PoisonError::into_inner);
        (
            info.cwd.clone(),
            info.name.clone(),
            info.agent == vt_proto::agent::AgentKind::Generic,
        )
    };
    let why = input.get("why").and_then(|w| w.as_str()).map(str::to_owned);
    let decision = decide(
        registry,
        server,
        tag,
        session,
        &name,
        &cwd,
        generic,
        tool_use,
        command,
        why.as_deref(),
    );
    let command = match decision {
        Decision::Allow { updated_input } => updated_input
            .as_ref()
            .and_then(|u| u.get("command"))
            .and_then(|c| c.as_str())
            .map_or_else(|| command.to_owned(), str::to_owned),
        Decision::Deny { reason } => {
            server.broadcast(
                AI_TOOL_RESULT,
                tag(serde_json::json!({ "tool_use": tool_use, "denied": true, "reason": reason })),
            );
            return error(format!("denied: {reason}"));
        }
    };
    match execute(registry, handle, session, &command) {
        Ok((exit, output)) => {
            server.broadcast(
                AI_TOOL_RESULT,
                tag(serde_json::json!({ "tool_use": tool_use, "command": command, "exit": exit, "output": output })),
            );
            Content::ToolResult {
                tool_use_id: tool_use.to_owned(),
                content: format!("exit {exit}\n{output}"),
                is_error: exit != 0,
            }
        }
        Err(message) => {
            server.broadcast(
                AI_TOOL_RESULT,
                tag(serde_json::json!({ "tool_use": tool_use, "command": command, "error": message })),
            );
            error(message)
        }
    }
}

/// Classify, evaluate the policy, announce, and either let a rule decide
/// (autonomy on, not dry-run) or wait for the inbox.
#[allow(clippy::too_many_arguments)] // one call site; the pieces are the request's own facts
fn decide(
    registry: &Arc<Registry>,
    server: &vt_ipc::Server,
    tag: &dyn Fn(serde_json::Value) -> Option<serde_json::Value>,
    session: &SessionId,
    name: &str,
    cwd: &std::path::Path,
    generic: bool,
    tool_use: &str,
    command: &str,
    why: Option<&str>,
) -> Decision {
    let classified = crate::policy::classify(command, cwd, false);
    let effective = crate::policy::effective(crate::policy::git_root(cwd).as_deref());
    let ctx = crate::policy::context(cwd, &effective);
    let repo = ctx
        .worktree
        .as_ref()
        .and_then(|w| w.file_name())
        .map(|n| n.to_string_lossy().into_owned());
    let outcome = vt_policy::evaluate(
        &effective.policy,
        &vt_policy::ToolRequest {
            tool: "Bash",
            command: Some(command),
            path: None,
            repo: repo.as_deref(),
            context: &ctx,
            generic_adapter: generic,
        },
    );
    let approval = ApprovalId(format!("agent-{tool_use}-{}", request_id()));
    let by_rule = outcome.applied;
    server.broadcast(
        AI_TOOL_REQUEST,
        tag(serde_json::json!({
            "tool_use": tool_use,
            "command": command,
            "why": why,
            "verdict": classified.verdict,
            "floor": classified.floor,
            "decision": outcome.decision,
            "rule": outcome.rule,
            "applied": by_rule,
            "approval": if by_rule { serde_json::Value::Null } else { serde_json::Value::String(approval.0.clone()) },
        })),
    );
    if by_rule {
        match outcome.decision {
            vt_policy::Decide::Allow => Decision::Allow {
                updated_input: None,
            },
            _ => Decision::Deny {
                reason: format!("policy rule `{}`", outcome.rule.unwrap_or_default()),
            },
        }
    } else {
        let Some(agents) = registry.agents() else {
            return Decision::Deny {
                reason: "the inbox is not ready".into(),
            };
        };
        let pending = agents.ask(InboxItem {
            id: approval,
            session: session.clone(),
            session_name: name.to_owned(),
            request: ApprovalRequest {
                id: ApprovalId(tool_use.to_owned()),
                tool: "Bash".into(),
                input: serde_json::json!({ "command": command, "why": why }),
                reason: why.map(str::to_owned),
                source: "agent-mode".into(),
            },
            hook_event: "agent_mode".into(),
            requested_at: now(),
            waiting_secs: 0,
            prompt_shown: false,
            reminders: 0,
            verdict: Some(classified.verdict.clone()),
            floor: classified.floor.clone(),
        });
        match Agents::wait_decision(&pending) {
            Some(d) => d,
            None => Decision::Deny {
                reason: "withdrawn before a decision".into(),
            },
        }
    }
}

/// Types the command at the prompt and waits for its block; the output
/// is redacted before it can leave, and withheld if redaction fails.
fn execute(
    registry: &Arc<Registry>,
    handle: &SessionHandle,
    session: &SessionId,
    command: &str,
) -> Result<(i32, String), String> {
    // A prompt mid-redraw reads as "not at the prompt" for a moment; wait
    // a little before calling the session busy.
    let settled = Instant::now();
    loop {
        let (tx, rx) = std::sync::mpsc::channel();
        handle
            .cmd
            .send(SessionCmd::AtPrompt(tx))
            .map_err(|_| "the session is gone".to_owned())?;
        let at_prompt = rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| "the session did not answer".to_owned())?;
        if at_prompt {
            break;
        }
        if settled.elapsed() > Duration::from_secs(3) {
            return Err(
                "the session is busy: a command is still running, so nothing was typed".into(),
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let last_seq = registry
        .store()
        .lock()
        .ok()
        .and_then(|s| s.blocks(session, 0, 1_000_000).ok())
        .and_then(|b| b.last().map(|b| b.seq))
        .unwrap_or(0);
    let mut bytes = command.as_bytes().to_vec();
    bytes.push(b'\r');
    handle
        .cmd
        .send(SessionCmd::Input(bytes))
        .map_err(|_| "the session is gone".to_owned())?;
    let start = Instant::now();
    let block = loop {
        if start.elapsed() > COMMAND_TIMEOUT {
            return Err(format!(
                "`{command}` is still running after {}s; its output is in the session",
                COMMAND_TIMEOUT.as_secs()
            ));
        }
        let found = registry
            .store()
            .lock()
            .ok()
            .and_then(|s| s.blocks(session, last_seq, 100).ok())
            .and_then(|blocks| {
                blocks.into_iter().find(|b| {
                    matches!(
                        b.block.kind,
                        vt_blocks::BlockKind::Command { exit: Some(_), .. }
                    )
                })
            });
        if let Some(b) = found {
            break b.block;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let vt_blocks::BlockKind::Command { exit, .. } = &block.kind else {
        unreachable!("filtered above");
    };
    let from = block.output_line.unwrap_or(block.start_line);
    let to = block.end_line.unwrap_or(from);
    let raw = if to >= from {
        Registry::export(handle, from, to, vt_core::core::TextFormat::Plain).unwrap_or_default()
    } else {
        String::new()
    };
    let max = usize::try_from(registry.cfg().ai.context.max_bytes).unwrap_or(8000);
    let trimmed = raw.trim_end();
    let clipped = if trimmed.len() > max {
        let cut = trimmed.len() - max;
        let mut at = cut;
        while !trimmed.is_char_boundary(at) {
            at += 1;
        }
        format!("[… {cut} bytes truncated]\n{}", &trimmed[at..])
    } else {
        trimmed.to_owned()
    };
    let clean = vt_redact::redact(&clipped)
        .map_err(|e| format!("output withheld: redaction failed: {e}"))?;
    Ok((exit.unwrap_or(-1), clean.text))
}
