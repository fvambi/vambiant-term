//! `vterm` — ephemeral CLI, a thin JSON-RPC client of `vtermd`.
//!
//! Planned commands (M2+): `ls`, `new`, `attach`, `kill`, `rename`, `logs`,
//! `inbox list|allow|deny|edit`, `task`, `ai`, `egress`, `audit`, `prune`
//! — every one with `--json`. Works inside Ghostty long before the GUI
//! exists; that is the point of M2.

fn main() {
    println!(
        "vterm {} (M0 scaffold; CLI lands in M2)",
        env!("CARGO_PKG_VERSION")
    );
}
