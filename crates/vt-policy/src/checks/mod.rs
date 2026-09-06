//! The per-command and per-pipeline checks behind [`crate::classify`].
//! Each check pushes a [`Finding`] that names the rule and the token, so
//! a verdict can be interrogated (docs/05 §5.2: "verdicts explain
//! themselves").

use crate::classify::{Context, Finding, SafetyClass, resolve};
use crate::paths::{is_secret_var, is_system_path};
mod access;
mod destructive;
mod wrappers;

use access::{cloud, credential_files, egress, permissions};
use destructive::{destructive_targets, find, git, redirects, remove, sql};
use wrappers::{eval, inline_code, nested, shell_c};

use crate::shell::{self, Pipeline, Simple, Word};

pub(super) const SHELLS: &[&str] = &["sh", "bash", "zsh", "dash", "ksh", "fish", "csh", "tcsh"];
pub(super) const INTERPRETERS: &[&str] = &["python", "python3", "perl", "ruby", "node", "php"];
pub(super) const FETCHERS: &[&str] = &[
    "curl", "wget", "nc", "ncat", "netcat", "http", "https", "aria2c", "fetch",
];
pub(super) const DECODERS: &[&str] = &[
    "base64", "openssl", "xxd", "uudecode", "gunzip", "zcat", "gzip", "bzcat", "xz", "unxz",
];
pub(super) const READERS: &[&str] = &[
    "cat", "less", "more", "head", "tail", "bat", "grep", "egrep", "fgrep", "rg", "ag", "strings",
    "xxd", "hexdump", "base64", "od", "cut", "awk", "sed", "sort", "uniq", "wc", "open", "pbcopy",
    "cp", "scp", "vim", "vi", "nano", "code", "source", ".",
];
pub(super) const EGRESS: &[&str] = &[
    "curl",
    "wget",
    "http",
    "https",
    "aria2c",
    "nc",
    "ncat",
    "netcat",
    "telnet",
    "ftp",
    "sftp",
    "scp",
    "ssh",
    "rsync",
    "ssh-copy-id",
    "socat",
];

pub(super) fn finding(
    class: SafetyClass,
    rule: &str,
    token: impl Into<String>,
    detail: impl Into<String>,
) -> Finding {
    Finding {
        class,
        rule: rule.into(),
        token: token.into(),
        detail: detail.into(),
        outside_worktree: false,
        protected_branch: false,
        host: None,
    }
}

pub(super) fn basename(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

/// Literal arguments after the command name; expansions come out as their `$` text.
pub(super) fn args(cmd: &Simple) -> Vec<String> {
    cmd.words.iter().skip(1).map(Word::text).collect()
}

/// Non-option arguments (`--` ends options).
pub(super) fn operands(list: &[String]) -> Vec<&str> {
    let mut out = Vec::new();
    let mut done = false;
    for a in list {
        if done || !a.starts_with('-') || a == "-" {
            out.push(a.as_str());
        } else if a == "--" {
            done = true;
        }
    }
    out
}

pub(super) fn has_flag(list: &[String], long: &str, short: Option<char>) -> bool {
    list.iter().any(|a| {
        a == long
            || short
                .is_some_and(|c| a.starts_with('-') && !a.starts_with("--") && a[1..].contains(c))
    })
}

/// Pipeline-level obfuscation: something fetched or decoded, fed to a shell.
pub(crate) fn pipeline(pipe: &Pipeline, findings: &mut Vec<Finding>) {
    let names: Vec<Option<String>> = pipe
        .commands
        .iter()
        .map(|c| match c {
            shell::Command::Simple(s) => s
                .words
                .first()
                .and_then(Word::literal)
                .map(|n| basename(&n).to_owned()),
            _ => None,
        })
        .collect();
    for (i, name) in names.iter().enumerate() {
        let Some(name) = name else { continue };
        let is_shell = SHELLS.contains(&name.as_str())
            || (INTERPRETERS.contains(&name.as_str()) && {
                let shell::Command::Simple(s) = &pipe.commands[i] else {
                    unreachable!()
                };
                let a = args(s);
                a.is_empty() || a.iter().all(|x| x == "-" || x.starts_with('-'))
            });
        if !is_shell || i == 0 {
            continue;
        }
        for earlier in names[..i].iter().flatten() {
            if FETCHERS.contains(&earlier.as_str()) || DECODERS.contains(&earlier.as_str()) {
                findings.push(finding(
                    SafetyClass::Obfuscated,
                    "pipe-to-shell",
                    format!("{earlier} | {name}"),
                    "output of a download or decoder is executed by a shell without being seen",
                ));
                return;
            }
            if let shell::Command::Simple(s) = &pipe.commands[i - 1]
                && (earlier == "printf" || earlier == "echo")
                && s.words.iter().any(|w| w.text().contains("\\x"))
            {
                findings.push(finding(
                    SafetyClass::Obfuscated,
                    "escaped-bytes-to-shell",
                    format!("{earlier} | {name}"),
                    "hex-escaped bytes are executed by a shell",
                ));
                return;
            }
        }
    }
}

/// Every check for one simple command.
pub(crate) fn simple(cmd: &Simple, ctx: &Context, findings: &mut Vec<Finding>, depth: usize) {
    redirects(cmd, ctx, findings);
    let Some(first) = cmd.words.first() else {
        return;
    };
    let Some(name) = first.literal() else {
        findings.push(finding(
            SafetyClass::Obfuscated,
            "verb-from-expansion",
            first.text(),
            "the command name comes from a variable or substitution and cannot be read",
        ));
        return;
    };
    let name = basename(&name).to_owned();
    let list = args(cmd);
    sql(&list, findings);
    if !execution(&name, cmd, &list, ctx, findings, depth) {
        files(&name, &list, ctx, findings);
        services_and_secrets(&name, cmd, &list, ctx, findings);
    }
    egress(&name, &list, ctx, findings);
}

/// Commands that run other commands or print the environment. Returns
/// true when `name` was one of them.
pub(super) fn execution(
    name: &str,
    cmd: &Simple,
    list: &[String],
    ctx: &Context,
    findings: &mut Vec<Finding>,
    depth: usize,
) -> bool {
    match name {
        "sudo" | "doas" | "pkexec" => {
            findings.push(finding(
                SafetyClass::Privilege,
                "sudo",
                name,
                "runs as another user, usually root",
            ));
            let inner = list
                .iter()
                .position(|a| !a.starts_with('-') && !a.contains('='))
                .map(|i| &list[i..]);
            if let Some(inner) = inner {
                nested(&inner.join(" "), ctx, findings, depth);
            }
        }
        "su" => findings.push(finding(SafetyClass::Privilege, "su", "su", "switches user")),
        "env" | "nohup" | "time" | "nice" | "command" | "exec" | "builtin" | "timeout"
        | "caffeinate" | "watch" | "xargs" | "stdbuf" => {
            let rest: Vec<String> = list
                .iter()
                .skip_while(|a| {
                    a.starts_with('-')
                        || a.contains('=')
                        || ((name == "timeout" || name == "nice")
                            && a.chars().all(|c| c.is_ascii_digit()))
                })
                .cloned()
                .collect();
            if name == "env" && rest.is_empty() {
                findings.push(finding(
                    SafetyClass::CredentialRead,
                    "env-dump",
                    "env",
                    "prints every environment variable, including secrets",
                ));
                return true;
            }
            if !rest.is_empty() {
                nested(&rest.join(" "), ctx, findings, depth);
            }
        }
        "printenv" => findings.push(finding(
            SafetyClass::CredentialRead,
            "env-dump",
            "printenv",
            "prints environment variables, including secrets",
        )),
        "set" | "export" | "declare" | "typeset"
            if list.is_empty() || list == ["-p"] || list == ["-x"] =>
        {
            findings.push(finding(
                SafetyClass::CredentialRead,
                "env-dump",
                name,
                "prints every variable, including secrets",
            ));
        }
        "eval" => eval(cmd, list, ctx, findings, depth),
        n if SHELLS.contains(&n) => shell_c(cmd, list, ctx, findings, depth),
        n if INTERPRETERS.contains(&n) => inline_code(n, list, findings),
        _ => return false,
    }
    true
}

/// Files, devices and git history.
pub(super) fn files(name: &str, list: &[String], ctx: &Context, findings: &mut Vec<Finding>) {
    match name {
        "rm" | "rmdir" | "shred" | "unlink" => remove(name, list, ctx, findings),
        "truncate" => {
            let targets: Vec<&str> = operands(list)
                .into_iter()
                .filter(|a| {
                    !a.chars()
                        .all(|c| c.is_ascii_digit() || "+-kKmMgG".contains(c))
                })
                .collect();
            destructive_targets(
                "truncate",
                "truncate",
                &targets,
                ctx,
                findings,
                "empties the file",
            );
        }
        "dd" => {
            let targets: Vec<&str> = list.iter().filter_map(|a| a.strip_prefix("of=")).collect();
            if !targets.is_empty() {
                destructive_targets(
                    "dd",
                    "dd-of",
                    &targets,
                    ctx,
                    findings,
                    "overwrites the output file or device",
                );
            }
        }
        n if n.starts_with("mkfs")
            || n.starts_with("newfs")
            || n == "wipefs"
            || n == "fdisk"
            || n == "parted" =>
        {
            let targets = operands(list);
            destructive_targets(
                n,
                "format",
                &targets,
                ctx,
                findings,
                "formats or repartitions a device",
            );
        }
        "diskutil" => {
            if list.iter().any(|a| {
                let l = a.to_ascii_lowercase();
                l.starts_with("erase")
                    || l == "partitiondisk"
                    || l == "zerodisk"
                    || l == "reformat"
                    || l == "secureerase"
            }) {
                let targets = operands(list);
                destructive_targets(
                    "diskutil",
                    "format",
                    &targets,
                    ctx,
                    findings,
                    "erases a disk",
                );
            }
        }
        "git" => git(list, ctx, findings),
        "find" => find(list, ctx, findings),
        _ => {}
    }
}

/// Permissions, services, system paths, secrets, cloud tools.
pub(super) fn services_and_secrets(
    name: &str,
    cmd: &Simple,
    list: &[String],
    ctx: &Context,
    findings: &mut Vec<Finding>,
) {
    match name {
        "chmod" | "chown" | "chgrp" => permissions(name, list, ctx, findings),
        "launchctl" | "systemctl" | "csrutil" | "spctl" | "nvram" | "dscl" | "visudo"
        | "usermod" | "useradd" | "userdel" | "passwd" | "sysctl" => {
            let sub = operands(list).first().copied().unwrap_or("");
            if name != "launchctl" && name != "systemctl"
                || [
                    "load",
                    "bootstrap",
                    "unload",
                    "bootout",
                    "enable",
                    "disable",
                    "start",
                    "stop",
                    "mask",
                ]
                .contains(&sub)
            {
                findings.push(finding(
                    SafetyClass::Privilege,
                    "system-service",
                    format!("{name} {sub}").trim().to_owned(),
                    "changes system services or settings",
                ));
            }
        }
        "cp" | "mv" | "install" | "tee" | "ln" => {
            let ops = operands(list);
            if let Some(last) = ops.last()
                && is_system_path(&resolve(last, ctx))
            {
                findings.push(finding(
                    SafetyClass::Privilege,
                    "write-system-path",
                    *last,
                    "writes into a system directory",
                ));
            }
            if READERS.contains(&name) {
                credential_files(name, list, findings);
            }
        }
        "security" => {
            if list
                .iter()
                .any(|a| a.contains("password") || a == "export" || a == "dump-keychain")
            {
                findings.push(finding(
                    SafetyClass::CredentialRead,
                    "keychain-read",
                    format!("security {}", list.first().map_or("", String::as_str)),
                    "reads a Keychain item",
                ));
            }
        }
        "op" | "vault" | "gh" | "aws" | "gcloud" | "az" | "kubectl" | "terraform" | "pulumi"
        | "helm" | "docker" | "npm" | "cargo" | "gem" | "twine" | "flyctl" | "fly" | "vercel" => {
            cloud(name, list, findings);
        }
        "echo" | "printf" => {
            for w in cmd.words.iter().skip(1) {
                for part in &w.parts {
                    if let shell::Part::Var(v) = part
                        && is_secret_var(v)
                    {
                        findings.push(finding(
                            SafetyClass::CredentialRead,
                            "echo-secret-var",
                            format!("${v}"),
                            "prints a variable whose name says it holds a secret",
                        ));
                    }
                }
            }
        }
        n if READERS.contains(&n) => credential_files(n, list, findings),
        _ => {}
    }
}
