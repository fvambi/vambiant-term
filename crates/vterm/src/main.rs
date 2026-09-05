//! `vterm` — ephemeral CLI, a thin JSON-RPC client of `vtermd` (ADR-0004).
//!
//! Every listing command has `--json`. `attach` renders a session in the
//! current terminal (Ghostty today) and forwards raw input, which is what
//! makes M2 a daily driver before the GUI exists.

mod attach;
mod cli;

use std::path::PathBuf;

use clap::Parser;

use cli::{Cli, Command, DaemonCmd};

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

fn main() {
    let cli = Cli::parse();
    match &cli.command {
        Command::Daemon { cmd } => daemon(&cli, cmd),
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
            for s in sessions {
                let state = if s.orphaned {
                    "orphaned".to_string()
                } else {
                    format!("{:?}", s.state).to_lowercase()
                };
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
        }
        Command::New {
            name,
            cwd,
            cols,
            rows,
            json,
            argv,
        } => {
            let mut c = client(&cli);
            let params = vt_proto::session::NewSession {
                name: name.clone(),
                argv: argv.clone(),
                cwd: cwd.clone().or_else(|| std::env::current_dir().ok()),
                env: Vec::new(),
                size: Some((*cols, *rows)),
                agent: None,
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
        Command::Send { session, text } => {
            let mut c = client(&cli);
            use base64::Engine as _;
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
            // Foreground start for now; the launchd plist lands with `vterm daemon install`.
            let exe = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join("vtermd")));
            let exe = exe
                .filter(|p| p.exists())
                .unwrap_or_else(|| PathBuf::from("vtermd"));
            let mut cmd = std::process::Command::new(exe);
            cmd.arg("--socket").arg(socket(cli));
            cmd.stdin(std::process::Stdio::null());
            match cmd.spawn() {
                Ok(child) => println!("vtermd started (pid {})", child.id()),
                Err(e) => fail(format!("cannot start vtermd: {e}")),
            }
        }
    }
}
