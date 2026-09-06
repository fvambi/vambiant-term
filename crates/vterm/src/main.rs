//! `vterm` — ephemeral CLI, a thin JSON-RPC client of `vtermd` (ADR-0004).
//!
//! Every listing command has `--json`. `attach` renders a session in the
//! current terminal (Ghostty today) and forwards raw input, which is what
//! makes M2 a daily driver before the GUI exists.

#![allow(unsafe_code)] // launchd: getuid; attach: termios

mod attach;
mod cli;
mod launchd;

use std::path::PathBuf;

use base64::Engine as _;
use clap::Parser;

use cli::{Cli, Command, ConfigCmd, DaemonCmd, EgressCmd, InboxCmd, ThemeCmd, WorkflowCmd};

fn socket(cli: &Cli) -> PathBuf {
    cli.socket
        .clone()
        .unwrap_or_else(vt_ipc::transport::socket_path)
}

fn client(cli: &Cli) -> vt_ipc::Client {
    match vt_ipc::Client::connect(&socket(cli)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("vterm: {e}");
            std::process::exit(2);
        }
    }
}

fn fail(e: impl std::fmt::Display) -> ! {
    eprintln!("vterm: {e}");
    std::process::exit(1)
}

#[allow(clippy::too_many_lines)] // one arm per subcommand
fn main() {
    let cli = Cli::parse();
    match &cli.command {
        Command::Daemon { cmd } => daemon(&cli, cmd),
        Command::Config { cmd } => config(&cli, cmd),
        Command::Keys { json } => keys(&cli, *json),
        Command::Blocks { session, json } => blocks(&cli, session, *json),
        Command::Ask {
            prompt,
            session,
            feature,
            json,
        } => ask(&cli, &prompt.join(" "), session.as_deref(), feature, *json),
        Command::Ai { cmd } => ai(&cli, cmd),
        Command::Classify {
            command,
            session,
            cwd,
            json,
        } => classify(
            &cli,
            &command.join(" "),
            session.as_deref(),
            cwd.as_deref(),
            *json,
        ),
        Command::Ls { json } => {
            let mut c = client(&cli);
            let v = c
                .call(vt_proto::session::method::SESSION_LIST, None)
                .unwrap_or_else(|e| fail(e));
            if *json {
                println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
                return;
            }
            let sessions: Vec<vt_proto::session::SessionInfo> =
                serde_json::from_value(v).unwrap_or_default();
            if sessions.is_empty() {
                println!("no sessions (start one with `vterm new`)");
            }
            if sessions.iter().any(|s| s.readopted) {
                println!("* re-adopted after a daemon restart: grid rebuilt from buffered output");
            }
            let degraded: Vec<(String, String)> = sessions
                .iter()
                .filter_map(|s| s.degraded.clone().map(|d| (s.name.clone(), d)))
                .collect();
            for s in sessions {
                let mut state = if s.orphaned {
                    "orphaned".to_string()
                } else if s.readopted {
                    format!("{:?}*", s.state).to_lowercase()
                } else {
                    format!("{:?}", s.state).to_lowercase()
                };
                if s.degraded.is_some() {
                    state.push('!');
                }
                let size = s.size.map_or(String::new(), |(c, r)| format!("{c}x{r}"));
                println!(
                    "{:<14} {:<20} {:<10} {:<8} {:>6} {}",
                    s.id.0,
                    s.name,
                    state,
                    size,
                    s.pid.map_or("-".into(), |p| p.to_string()),
                    s.cwd.display()
                );
            }
            for (name, why) in degraded {
                println!("! {name}: degraded — {why}");
            }
        }
        Command::New {
            name,
            cwd,
            cols,
            rows,
            json,
            agent,
            argv,
        } => {
            let mut c = client(&cli);
            let params = vt_proto::session::NewSession {
                name: name.clone(),
                argv: argv.clone(),
                cwd: cwd.clone().or_else(|| std::env::current_dir().ok()),
                env: Vec::new(),
                size: Some((*cols, *rows)),
                agent: match agent.as_deref() {
                    None => None,
                    Some("claude") => Some(vt_proto::agent::AgentKind::Claude),
                    Some("codex") => Some(vt_proto::agent::AgentKind::Codex),
                    Some(other) => fail(format!("unknown agent `{other}` (claude, codex)")),
                },
            };
            let v = c
                .call(
                    vt_proto::session::method::SESSION_NEW,
                    serde_json::to_value(params).ok(),
                )
                .unwrap_or_else(|e| fail(e));
            if *json {
                println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            } else {
                let info: vt_proto::session::SessionInfo =
                    serde_json::from_value(v).unwrap_or_else(|e| fail(e));
                println!("{} {}", info.id.0, info.name);
            }
        }
        Command::Attach { session } => {
            if let Err(e) = attach::run(&socket(&cli), session) {
                fail(e);
            }
        }
        Command::Kill { session, signal } => {
            let mut c = client(&cli);
            c.call(
                vt_proto::session::method::SESSION_KILL,
                Some(serde_json::json!({ "id": session, "signal": signal })),
            )
            .unwrap_or_else(|e| fail(e));
        }
        Command::Rename { session, name } => {
            let mut c = client(&cli);
            c.call(
                vt_proto::session::method::SESSION_RENAME,
                Some(serde_json::json!({ "id": session, "name": name })),
            )
            .unwrap_or_else(|e| fail(e));
        }
        Command::Logs {
            session,
            lines,
            json,
        } => {
            let mut c = client(&cli);
            let v = c
                .call(
                    vt_proto::session::method::SESSION_LOGS,
                    Some(serde_json::json!({ "id": session, "lines": lines })),
                )
                .unwrap_or_else(|e| fail(e));
            if *json {
                println!("{v}");
            } else {
                println!("{}", v.get("text").and_then(|t| t.as_str()).unwrap_or(""));
            }
        }
        Command::Inbox { cmd } => inbox(&cli, cmd),
        Command::Egress { cmd } => egress(&cli, cmd),
        Command::Theme { cmd } => theme(&cli, cmd),
        Command::Workflow { cmd } => workflow(&cli, cmd),
        Command::Events {
            session,
            after,
            json,
        } => {
            let mut c = client(&cli);
            let v = c
                .call(
                    "agent.events",
                    Some(serde_json::json!({ "id": session, "after": after, "limit": 500 })),
                )
                .unwrap_or_else(|e| fail(e));
            if *json {
                println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
                return;
            }
            for entry in v.as_array().cloned().unwrap_or_default() {
                let seq = entry
                    .get(0)
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0);
                let ev = entry.get(1).cloned().unwrap_or(serde_json::Value::Null);
                let kind = ev.get("type").and_then(|t| t.as_str()).unwrap_or("?");
                let detail = ev
                    .get("name")
                    .or_else(|| ev.get("tool"))
                    .or_else(|| ev.get("text"))
                    .or_else(|| ev.get("body"))
                    .or_else(|| ev.get("reason"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("");
                println!(
                    "{seq:>6} {kind:<18} {}",
                    detail
                        .chars()
                        .take(100)
                        .collect::<String>()
                        .replace('\n', " ")
                );
            }
        }
        Command::Statusline {
            receiver,
            token,
            subagent,
        } => {
            // Relay stdin JSON to the daemon and print a compact status line.
            let mut input = String::new();
            let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut input);
            let port: u16 = receiver
                .rsplit(':')
                .next()
                .and_then(|p| p.parse().ok())
                .unwrap_or(0);
            let path = if *subagent {
                format!("/status/{token}?subagent=1")
            } else {
                format!("/status/{token}")
            };
            let _ = vt_ipc::http::post_json(port, &path, input.as_bytes());
            if !*subagent {
                let v: serde_json::Value = serde_json::from_str(&input).unwrap_or_default();
                let model = v
                    .pointer("/model/display_name")
                    .and_then(|m| m.as_str())
                    .unwrap_or("");
                let cost = v
                    .pointer("/cost/total_cost_usd")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(0.0);
                let ctx = v
                    .pointer("/context_window/used_percentage")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(0.0);
                println!("{model} · ctx {ctx:.0}% · ${cost:.3} est · vterm");
            }
        }
        Command::Send { session, text } => {
            let mut c = client(&cli);
            let bytes = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
            c.call(
                vt_proto::session::method::SESSION_INPUT,
                Some(serde_json::json!({ "id": session, "bytes": bytes })),
            )
            .unwrap_or_else(|e| fail(e));
        }
    }
}

fn daemon(cli: &Cli, cmd: &DaemonCmd) {
    match cmd {
        DaemonCmd::Status { json } => match vt_ipc::Client::connect(&socket(cli)) {
            Ok(mut c) => {
                let v = c
                    .call(vt_proto::session::method::DAEMON_STATUS, None)
                    .unwrap_or_else(|e| fail(e));
                if *json {
                    println!("{v}");
                } else {
                    println!(
                        "vtermd {} running (pid {}), {} sessions, socket {}",
                        v.get("version").and_then(|x| x.as_str()).unwrap_or("?"),
                        v.get("pid")
                            .and_then(serde_json::Value::as_u64)
                            .unwrap_or(0),
                        v.get("sessions")
                            .and_then(serde_json::Value::as_u64)
                            .unwrap_or(0),
                        socket(cli).display()
                    );
                }
            }
            Err(e) => {
                if *json {
                    println!(
                        "{}",
                        serde_json::json!({ "running": false, "error": e.to_string() })
                    );
                } else {
                    println!("vtermd not running ({e})");
                }
                std::process::exit(3);
            }
        },
        DaemonCmd::Start => {
            if launchd::plist_path().exists() {
                match launchd::kickstart() {
                    Ok(()) => println!("vtermd started through launchd ({})", launchd::LABEL),
                    Err(e) => fail(e),
                }
                return;
            }
            let exe = launchd::vtermd_path(None);
            let mut cmd = std::process::Command::new(&exe);
            cmd.arg("--socket").arg(socket(cli));
            cmd.stdin(std::process::Stdio::null());
            match cmd.spawn() {
                Ok(child) => println!(
                    "vtermd started (pid {}, not launchd-managed — run `vterm daemon install` for KeepAlive)",
                    child.id()
                ),
                Err(e) => fail(format!("cannot start {}: {e}", exe.display())),
            }
        }
        DaemonCmd::Stop => match launchd::stop() {
            Ok(()) => println!("vtermd stopped; running sessions are now orphaned"),
            Err(e) => fail(e),
        },
        DaemonCmd::Install { vtermd } => match launchd::install(vtermd.as_deref()) {
            Ok(path) => println!(
                "installed {}; vtermd will start now and on login",
                path.display()
            ),
            Err(e) => fail(e),
        },
        DaemonCmd::Uninstall => match launchd::uninstall() {
            Ok(()) => println!("removed the vtermd LaunchAgent"),
            Err(e) => fail(e),
        },
    }
}

fn classify(cli: &Cli, command: &str, session: Option<&str>, cwd: Option<&str>, json: bool) {
    let mut c = client(cli);
    let mut params = serde_json::json!({ "command": command });
    if let Some(s) = session {
        params["session"] = serde_json::Value::String(s.to_owned());
    }
    if let Some(d) = cwd {
        params["cwd"] = serde_json::Value::String(d.to_owned());
    }
    let v = c
        .call(vt_proto::session::method::POLICY_CLASSIFY, Some(params))
        .unwrap_or_else(|e| fail(e));
    if json {
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return;
    }
    let class = v
        .pointer("/verdict/class")
        .and_then(|x| x.as_str())
        .unwrap_or("?");
    let decision = v.get("decision").and_then(|x| x.as_str()).unwrap_or("?");
    println!("{class}  ({decision})");
    for f in v
        .pointer("/verdict/findings")
        .and_then(|x| x.as_array())
        .into_iter()
        .flatten()
    {
        let rule = f.get("rule").and_then(|x| x.as_str()).unwrap_or("?");
        let token = f.get("token").and_then(|x| x.as_str()).unwrap_or("");
        let detail = f.get("detail").and_then(|x| x.as_str()).unwrap_or("");
        let outside = f
            .get("outside_worktree")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        println!(
            "  {rule}: `{token}` — {detail}{}",
            if outside {
                " (outside the worktree)"
            } else {
                ""
            }
        );
    }
    if let Some(floor) = v.get("floor") {
        let reason = floor.get("reason").and_then(|x| x.as_str()).unwrap_or("?");
        println!("  never auto-approved: {reason}");
    }
}

fn workflow(cli: &Cli, cmd: &WorkflowCmd) {
    let mut c = client(cli);
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let v = c
        .call(
            vt_proto::session::method::WORKFLOWS_LIST,
            Some(serde_json::json!({ "cwd": cwd })),
        )
        .unwrap_or_else(|e| fail(e));
    let list: Vec<vt_workflows::Workflow> =
        serde_json::from_value(v["workflows"].clone()).unwrap_or_default();
    match cmd {
        WorkflowCmd::List { json } => {
            if *json {
                println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
                return;
            }
            if list.is_empty() {
                println!(
                    "no workflows: put YAML files in ~/.config/vambiant-term/workflows or <repo>/.vambiant-term/workflows"
                );
            }
            for w in &list {
                let args: Vec<&str> = w.arguments.iter().map(|a| a.name.as_str()).collect();
                println!(
                    "{:<28} {}{}  [{}]",
                    w.name,
                    w.description,
                    if args.is_empty() {
                        String::new()
                    } else {
                        format!("  ({{{{{}}}}})", args.join("}} {{"))
                    },
                    if w.warp { "warp" } else { "vambiant" }
                );
            }
            for p in v["problems"].as_array().into_iter().flatten() {
                println!("  problem: {}", p.as_str().unwrap_or(""));
            }
        }
        WorkflowCmd::Show { name, args } => {
            let Some(w) = list.iter().find(|w| w.name.eq_ignore_ascii_case(name)) else {
                fail(format!("no workflow named `{name}`"));
            };
            let values: Vec<(String, String)> = args
                .iter()
                .filter_map(|a| a.split_once('=').map(|(k, v)| (k.to_owned(), v.to_owned())))
                .collect();
            println!("{}", w.render(&values));
        }
    }
}

fn theme(cli: &Cli, cmd: &ThemeCmd) {
    let mut c = client(cli);
    match cmd {
        ThemeCmd::Import { file, name, format } => {
            let path =
                std::fs::canonicalize(file).unwrap_or_else(|_| std::path::PathBuf::from(file));
            let mut params = serde_json::json!({ "path": path.display().to_string() });
            if let Some(n) = name {
                params["name"] = serde_json::Value::String(n.clone());
            }
            if let Some(f) = format {
                params["format"] = serde_json::Value::String(f.clone());
            }
            let v = c
                .call(vt_proto::session::method::CONFIG_THEME_IMPORT, Some(params))
                .unwrap_or_else(|e| fail(e));
            println!(
                "imported `{}` → {}",
                v["name"].as_str().unwrap_or("?"),
                v["path"].as_str().unwrap_or("?")
            );
            for w in v["warnings"].as_array().into_iter().flatten() {
                println!("  warning: {}", w.as_str().unwrap_or(""));
            }
            println!(
                "use it: vterm config set theme.name {}",
                v["name"].as_str().unwrap_or("?")
            );
        }
    }
}

fn egress(cli: &Cli, cmd: &EgressCmd) {
    let mut c = client(cli);
    match cmd {
        EgressCmd::Tail { limit, json } => {
            let v = c
                .call(
                    vt_proto::session::method::EGRESS_TAIL,
                    Some(serde_json::json!({ "limit": limit, "payload": json })),
                )
                .unwrap_or_else(|e| fail(e));
            if *json {
                println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
                return;
            }
            let rows = v.as_array().cloned().unwrap_or_default();
            if rows.is_empty() {
                println!("nothing has left the machine yet");
            }
            for r in rows {
                let s = |k: &str| r.get(k).and_then(|x| x.as_str()).unwrap_or("?").to_owned();
                let n = |k: &str| r.get(k).and_then(serde_json::Value::as_u64).unwrap_or(0);
                println!(
                    "{}  {:<8} {:<14} {:<22} {:>7} B  {} redaction{}",
                    s("at"),
                    s("purpose"),
                    s("provider"),
                    s("model"),
                    n("bytes_sent"),
                    n("redactions"),
                    if n("redactions") == 1 { "" } else { "s" }
                );
            }
        }
        EgressCmd::Last { session } => {
            let mut params = serde_json::json!({});
            if let Some(s) = session {
                params["session"] = serde_json::Value::String(s.clone());
            }
            let v = c
                .call(vt_proto::session::method::AI_PAYLOAD_LAST, Some(params))
                .unwrap_or_else(|e| fail(e));
            println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        }
    }
}

fn inbox(cli: &Cli, cmd: &InboxCmd) {
    let mut c = client(cli);
    match cmd {
        InboxCmd::List { json } => {
            let v = c.call("inbox.list", None).unwrap_or_else(|e| fail(e));
            if *json {
                println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
                return;
            }
            let items = v.as_array().cloned().unwrap_or_default();
            if items.is_empty() {
                println!("inbox empty");
            }
            for it in items {
                let id = it.get("id").and_then(|x| x.as_str()).unwrap_or("?");
                let name = it
                    .get("session_name")
                    .and_then(|x| x.as_str())
                    .unwrap_or("?");
                let tool = it
                    .pointer("/request/tool")
                    .and_then(|x| x.as_str())
                    .unwrap_or("?");
                let waiting = it
                    .get("waiting_secs")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                let shown = it
                    .get("prompt_shown")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                let reminders = it
                    .get("reminders")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                let input = it
                    .pointer("/request/input")
                    .map(std::string::ToString::to_string)
                    .unwrap_or_default();
                println!(
                    "{id}\n  session {name} · {tool} · waiting {waiting}s{}{}\n  {}",
                    if shown {
                        " · prompt shown in terminal"
                    } else {
                        ""
                    },
                    if reminders > 0 {
                        format!(" · reminded ×{reminders}")
                    } else {
                        String::new()
                    },
                    input.chars().take(160).collect::<String>()
                );
            }
        }
        InboxCmd::Allow { id } => {
            decide_all(&mut c, id, &serde_json::json!({ "behavior": "allow" }));
        }
        InboxCmd::Deny { id, reason } => {
            decide_all(
                &mut c,
                id,
                &serde_json::json!({ "behavior": "deny", "reason": reason }),
            );
        }
        InboxCmd::Edit { id, input } => {
            let v = c.call("inbox.list", None).unwrap_or_else(|e| fail(e));
            let Some(item) = v.as_array().and_then(|a| {
                a.iter()
                    .find(|i| i.get("id").and_then(|x| x.as_str()) == Some(id))
            }) else {
                fail(format!(
                    "no pending approval `{id}` (see `vterm inbox list`)"
                ))
            };
            let current = item
                .pointer("/request/input")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            let edited = match input {
                Some(text) => serde_json::from_str::<serde_json::Value>(text)
                    .unwrap_or_else(|e| fail(format!("--input is not JSON: {e}"))),
                None => edit_in_editor(&current),
            };
            if edited == current {
                println!("unchanged; use `vterm inbox allow {id}` to allow as-is");
                return;
            }
            let decision = serde_json::json!({ "behavior": "allow", "updated_input": edited });
            c.call(
                "inbox.decide",
                Some(serde_json::json!({ "id": id, "decision": decision })),
            )
            .unwrap_or_else(|e| fail(e));
            println!("{id}: allow with edited input");
        }
    }
}

/// Open `$VISUAL` / `$EDITOR` (or `vi`) on the pretty-printed input and read
/// it back; exits with an explanation if the result is not JSON.
fn edit_in_editor(current: &serde_json::Value) -> serde_json::Value {
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".into());
    let path = std::env::temp_dir().join(format!("vterm-inbox-edit-{}.json", std::process::id()));
    std::fs::write(
        &path,
        serde_json::to_string_pretty(current).unwrap_or_default(),
    )
    .unwrap_or_else(|e| fail(format!("cannot write {}: {e}", path.display())));
    let status = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(format!("{editor} \"$1\""))
        .arg("sh")
        .arg(&path)
        .status()
        .unwrap_or_else(|e| fail(format!("cannot run editor `{editor}`: {e}")));
    if !status.success() {
        let _ = std::fs::remove_file(&path);
        fail(format!(
            "editor `{editor}` exited with {status}; nothing decided"
        ));
    }
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let _ = std::fs::remove_file(&path);
    serde_json::from_str(&text)
        .unwrap_or_else(|e| fail(format!("edited input is not JSON ({e}); nothing decided")))
}

fn decide_all(c: &mut vt_ipc::Client, id: &str, decision: &serde_json::Value) {
    let ids: Vec<String> = if id == "all" {
        let v = c.call("inbox.list", None).unwrap_or_else(|e| fail(e));
        v.as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|it| it.get("id").and_then(|x| x.as_str()).map(str::to_owned))
            .collect()
    } else {
        vec![id.to_owned()]
    };
    for id in ids {
        match c.call(
            "inbox.decide",
            Some(serde_json::json!({ "id": id, "decision": decision })),
        ) {
            Ok(_) => println!(
                "{id}: {}",
                decision
                    .get("behavior")
                    .and_then(|b| b.as_str())
                    .unwrap_or("?")
            ),
            Err(e) => eprintln!("vterm: {e}"),
        }
    }
}

fn config(cli: &Cli, cmd: &ConfigCmd) {
    use vt_proto::session::method;
    let mut c = client(cli);
    match cmd {
        ConfigCmd::Path => {
            let v = c.call(method::CONFIG_GET, None).unwrap_or_else(|e| fail(e));
            let paths = &v["paths"];
            for (label, key) in [
                ("config", "config"),
                ("keymap", "keymap"),
                ("themes", "themes"),
            ] {
                println!("{label:<7} {}", paths[key].as_str().unwrap_or("?"));
            }
            for (key, label) in [
                ("config_error", "config.toml"),
                ("keymap_error", "keymap.toml"),
            ] {
                if let Some(e) = v[key].as_object() {
                    println!(
                        "{label}: ERROR {}{}: {}",
                        e["file"].as_str().unwrap_or(""),
                        e["line"]
                            .as_u64()
                            .map_or(String::new(), |l| format!(":{l}")),
                        e["message"].as_str().unwrap_or("")
                    );
                }
            }
            for w in v["config_warnings"]
                .as_array()
                .into_iter()
                .flatten()
                .chain(v["theme_warnings"].as_array().into_iter().flatten())
            {
                println!("warning: {}", w.as_str().unwrap_or(""));
            }
        }
        ConfigCmd::Show { json } => {
            let v = c.call(method::CONFIG_GET, None).unwrap_or_else(|e| fail(e));
            if *json {
                println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            } else {
                let config: vt_config::Config =
                    serde_json::from_value(v["config"].clone()).unwrap_or_else(|e| fail(e));
                print!("{}", toml::to_string(&config).unwrap_or_default());
            }
        }
        ConfigCmd::Get { key } => {
            let v = c.call(method::CONFIG_GET, None).unwrap_or_else(|e| fail(e));
            let mut cur = &v["config"];
            for part in key.split('.') {
                cur = &cur[part];
            }
            if cur.is_null() {
                fail(format!("no such key: {key}"));
            }
            println!("{cur}");
        }
        ConfigCmd::Set { key, value } => {
            let value: serde_json::Value = serde_json::from_str(value)
                .unwrap_or_else(|_| serde_json::Value::String(value.clone()));
            let params = serde_json::json!({ "key": key, "value": value });
            c.call(method::CONFIG_SET, Some(params))
                .unwrap_or_else(|e| fail(e));
            println!("{key} = {value}");
        }
        ConfigCmd::Bind {
            chord,
            action,
            remove,
        } => {
            let params = if *remove {
                serde_json::json!({ "chord": chord })
            } else {
                let Some(action) = action else {
                    fail("an action is required unless --remove is given")
                };
                serde_json::json!({ "chord": chord, "action": action })
            };
            let v = c
                .call(method::CONFIG_KEYMAP_SET, Some(params))
                .unwrap_or_else(|e| fail(e));
            print_keys(&v);
        }
    }
}

fn keys(cli: &Cli, json: bool) {
    let mut c = client(cli);
    let v = c
        .call(vt_proto::session::method::CONFIG_GET, None)
        .unwrap_or_else(|e| fail(e));
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&v["keymap"]).unwrap_or_default()
        );
    } else {
        print_keys(&v["keymap"]);
    }
}

fn print_keys(keymap: &serde_json::Value) {
    println!(
        "profile {}  prefix {}",
        keymap["profile"].as_str().unwrap_or("?"),
        keymap["prefix"].as_str().unwrap_or("?")
    );
    for b in keymap["bindings"].as_array().into_iter().flatten() {
        println!(
            "{:<22} {:<26} {}",
            b["chord"].as_str().unwrap_or(""),
            b["action"].as_str().unwrap_or(""),
            b["source"].as_str().unwrap_or("")
        );
    }
    for e in keymap["errors"].as_array().into_iter().flatten() {
        println!("error: {}", e.as_str().unwrap_or(""));
    }
    for e in keymap["conflicts"].as_array().into_iter().flatten() {
        println!("conflict: {}", e.as_str().unwrap_or(""));
    }
}

fn blocks(cli: &Cli, session: &str, json: bool) {
    let mut c = client(cli);
    let v = c
        .call(
            vt_proto::session::method::SESSION_BLOCKS,
            Some(serde_json::json!({ "id": session })),
        )
        .unwrap_or_else(|e| fail(e));
    if json {
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return;
    }
    let Some(arr) = v.as_array() else { return };
    if arr.is_empty() {
        println!(
            "no blocks (shell integration may be off; see `vterm config get shell_integration.enabled`)"
        );
        return;
    }
    for item in arr {
        let b = &item["block"];
        let mark = if b["confidence"] == "heuristic" {
            "\u{2248}"
        } else {
            " "
        };
        // `BlockKind` serialises tagged: `kind: {kind: "command", cmdline, exit}`.
        let kind = &b["kind"];
        match kind["kind"].as_str() {
            Some("command") => {
                let exit = kind["exit"].as_i64();
                let chip = exit.map_or_else(|| "· running".to_owned(), |e| format!("exit {e}"));
                let cmd = kind["cmdline"].as_str().unwrap_or("<no command line>");
                println!("{mark} [{chip}] {cmd}");
            }
            Some("background") => println!("\u{2248} (background output)"),
            _ => println!("{mark} (prompt)"),
        }
    }
}

fn ask(cli: &Cli, prompt: &str, session: Option<&str>, feature: &str, json: bool) {
    let mut c = client(cli);
    let v = c
        .call(
            vt_proto::session::method::AI_ASK,
            Some(serde_json::json!({ "prompt": prompt, "session": session, "feature": feature })),
        )
        .unwrap_or_else(|e| fail(e));
    if json {
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return;
    }
    println!("{}", v["text"].as_str().unwrap_or_default().trim_end());
    let cost = v["cost_usd_estimate"].as_f64().unwrap_or(0.0);
    let redactions = v["redactions"].as_u64().unwrap_or(0);
    eprintln!(
        "\n— {} · {} · in {} out {} tokens · est. ${cost:.4}{}",
        v["profile"].as_str().unwrap_or("?"),
        v["model"].as_str().unwrap_or("?"),
        v["usage"]["input_tokens"],
        v["usage"]["output_tokens"],
        if redactions > 0 {
            format!(" · {redactions} redaction(s) applied before sending")
        } else {
            String::new()
        }
    );
}

/// `vterm ai spend`: the egress log's costs and the budget (docs/04 §7).
fn ai_spend(cli: &Cli, since: Option<&str>, session: Option<&str>, json: bool) {
    let mut c = client(cli);
    let mut params = serde_json::json!({});
    if let Some(s) = since {
        params["since"] = serde_json::Value::String(s.to_owned());
    }
    if let Some(s) = session {
        params["session"] = serde_json::Value::String(s.to_owned());
    }
    let v = c
        .call(vt_proto::session::method::AI_SPEND, Some(params))
        .unwrap_or_else(|e| fail(e));
    if json {
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return;
    }
    let spend = &v["spend"];
    println!(
        "since {}: {} request(s), ≈${:.4} (list-price estimates)",
        v["since"].as_str().unwrap_or("?"),
        spend["requests"].as_u64().unwrap_or(0),
        spend["total_usd"].as_f64().unwrap_or(0.0)
    );
    for (label, key) in [("by purpose", "by_purpose"), ("by provider", "by_provider")] {
        for row in spend[key].as_array().into_iter().flatten() {
            println!(
                "  {label:<12} {:<16} ≈${:.4}",
                row[0].as_str().unwrap_or("?"),
                row[1].as_f64().unwrap_or(0.0)
            );
        }
    }
    let b = &v["budget"];
    println!(
        "budget: ≈${:.2} of ${:.2} today, ≈${:.2} of ${:.2} this month{}",
        b["daily_used"].as_f64().unwrap_or(0.0),
        b["daily_limit"].as_f64().unwrap_or(0.0),
        b["monthly_used"].as_f64().unwrap_or(0.0),
        b["monthly_limit"].as_f64().unwrap_or(0.0),
        if b["hard_stop"].as_bool().unwrap_or(true) {
            " (hard stop on)"
        } else {
            " (hard stop off)"
        }
    );
}

fn ai(cli: &Cli, cmd: &cli::AiCmd) {
    match cmd {
        cli::AiCmd::Spend {
            since,
            session,
            json,
        } => ai_spend(cli, since.as_deref(), session.as_deref(), *json),
        cli::AiCmd::Doctor { json } => {
            let mut c = client(cli);
            let v = c
                .call(vt_proto::session::method::AI_DOCTOR, None)
                .unwrap_or_else(|e| fail(e));
            if *json {
                println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
                return;
            }
            println!(
                "providers: {} ({})",
                v["providers_path"].as_str().unwrap_or("?"),
                if v["exists"].as_bool().unwrap_or(false) {
                    "file"
                } else {
                    "bundled defaults; write it with your profiles"
                }
            );
            for p in v["profiles"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default()
            {
                let key = if p["key"].as_bool().unwrap_or(false) {
                    "key ✓"
                } else {
                    "no key"
                };
                let reach = if p["reachable"].as_bool().unwrap_or(false) {
                    format!(
                        "reachable, {} model(s)",
                        p["models"].as_array().map_or(0, Vec::len)
                    )
                } else {
                    format!("unreachable: {}", p["error"].as_str().unwrap_or("?"))
                };
                println!(
                    "  {:<14} {:<10} {:<28} {key:<7} {reach}",
                    p["name"].as_str().unwrap_or("?"),
                    p["kind"].as_str().unwrap_or("?"),
                    p["model"].as_str().unwrap_or("?")
                );
                if let Some(h) = p["start_hint"]
                    .as_str()
                    .filter(|h| !h.is_empty() && !p["reachable"].as_bool().unwrap_or(false))
                {
                    println!("  {:<14} start it: {h}", "");
                }
            }
            println!("routes: {}", v["routes"]);
        }
        cli::AiCmd::Key { cmd } => match cmd {
            cli::KeyCmd::Set { profile } => {
                use std::io::Read as _;
                let mut key = String::new();
                if std::io::stdin().read_to_string(&mut key).is_err() || key.trim().is_empty() {
                    fail(format!(
                        "vterm ai key set {profile}: pipe the key on stdin, e.g. `pbpaste | vterm ai key set {profile}`"
                    ));
                }
                match vt_ai::keychain::store(profile, key.trim()) {
                    Ok(()) => println!("stored the key for `{profile}` in the Keychain"),
                    Err(e) => fail(e),
                }
            }
            cli::KeyCmd::Remove { profile } => match vt_ai::keychain::remove(profile) {
                Ok(()) => println!("removed the key for `{profile}`"),
                Err(e) => fail(e),
            },
        },
    }
}
