//! Permissions, credentials, cloud and deployment tools, network egress.

use super::{EGRESS, finding, operands};
use crate::classify::{Context, Finding, SafetyClass, resolve};
use crate::paths::{host_of, is_secret_path, is_system_path};

pub(super) fn permissions(name: &str, list: &[String], ctx: &Context, findings: &mut Vec<Finding>) {
    let ops = operands(list);
    let mode = ops.first().copied().unwrap_or("");
    let world = name == "chmod"
        && (mode == "777"
            || mode == "666"
            || mode.contains("o+w")
            || mode.contains("a+w")
            || mode.contains("a+rwx"));
    let root =
        name != "chmod" && (mode.starts_with("root") || mode.starts_with("0:") || mode == "0");
    let system = ops.iter().skip(1).any(|t| is_system_path(&resolve(t, ctx)));
    if world || root || system {
        findings.push(finding(
            SafetyClass::Privilege,
            if system {
                "system-permissions"
            } else {
                "world-writable"
            },
            format!("{name} {mode}"),
            "changes ownership or permissions in a way that weakens the system",
        ));
    }
}

pub(super) fn credential_files(name: &str, list: &[String], findings: &mut Vec<Finding>) {
    for t in operands(list) {
        if is_secret_path(t) {
            findings.push(finding(
                SafetyClass::CredentialRead,
                "read-secret-file",
                format!("{name} {t}"),
                "reads a file that holds credentials",
            ));
        }
    }
}

pub(super) fn cloud(name: &str, list: &[String], findings: &mut Vec<Finding>) {
    let ops = operands(list);
    let joined = ops.iter().take(3).copied().collect::<Vec<_>>().join(" ");
    let token = format!("{name} {joined}").trim().to_owned();
    let credential = match name {
        "op" => {
            matches!(ops.first().copied(), Some("read"))
                || (ops.first() == Some(&"item") && ops.get(1) == Some(&"get"))
        }
        "vault" => {
            matches!(ops.first().copied(), Some("read" | "login"))
                || (ops.first() == Some(&"kv") && ops.get(1) == Some(&"get"))
        }
        "gh" => ops.first() == Some(&"auth") && ops.get(1) == Some(&"token"),
        "aws" => {
            matches!(joined.as_str(), s if s.starts_with("configure get") || s.starts_with("configure export-credentials") || s.starts_with("sts get-session-token") || s.starts_with("secretsmanager get-secret-value") || (s.starts_with("ssm get-parameter") && list.iter().any(|a| a == "--with-decryption")))
        }
        "gcloud" => {
            joined.starts_with("auth print-access-token")
                || joined.starts_with("auth print-identity-token")
        }
        "az" => {
            joined.starts_with("account get-access-token")
                || joined.starts_with("keyvault secret show")
        }
        "kubectl" => {
            ops.first() == Some(&"get") && matches!(ops.get(1).copied(), Some("secret" | "secrets"))
        }
        _ => false,
    };
    if credential {
        findings.push(finding(
            SafetyClass::CredentialRead,
            "credential-tool",
            token,
            "fetches a credential",
        ));
        return;
    }
    let remote = match name {
        "gh" => {
            matches!(
                (ops.first().copied(), ops.get(1).copied()),
                (Some("release" | "repo"), Some("delete"))
            ) || (ops.first() == Some(&"api") && list.iter().any(|a| a == "DELETE"))
        }
        "terraform" => matches!(ops.first().copied(), Some("apply" | "destroy")),
        "pulumi" => matches!(ops.first().copied(), Some("up" | "destroy")),
        "kubectl" => matches!(
            ops.first().copied(),
            Some("delete" | "apply" | "replace" | "drain")
        ),
        "helm" => matches!(
            ops.first().copied(),
            Some("uninstall" | "delete" | "upgrade")
        ),
        "docker" => {
            matches!(ops.first().copied(), Some("rm" | "rmi"))
                || (ops.first() == Some(&"system") && ops.get(1) == Some(&"prune"))
                || (matches!(ops.first().copied(), Some("volume" | "container" | "image"))
                    && ops.get(1) == Some(&"rm"))
                || (ops.first() == Some(&"compose")
                    && ops.get(1) == Some(&"down")
                    && list.iter().any(|a| a == "-v" || a == "--volumes"))
        }
        "aws" => {
            ops.iter()
                .any(|a| a.starts_with("delete-") || *a == "terminate-instances" || *a == "rb")
                || (ops.first() == Some(&"s3") && ops.get(1) == Some(&"rm"))
        }
        "gcloud" | "az" => ops.contains(&"delete"),
        "npm" => matches!(
            ops.first().copied(),
            Some("publish" | "unpublish" | "deprecate")
        ),
        "cargo" => matches!(ops.first().copied(), Some("publish" | "yank")),
        "gem" => ops.first() == Some(&"push"),
        "twine" => ops.first() == Some(&"upload"),
        "fly" | "flyctl" => ops.first() == Some(&"deploy"),
        "vercel" => ops.first() == Some(&"deploy") || list.iter().any(|a| a == "--prod"),
        _ => false,
    };
    if remote {
        findings.push(finding(
            SafetyClass::IrreversibleRemote,
            "remote-mutation",
            token,
            "changes or removes something on a remote system",
        ));
    }
}

pub(super) fn egress(name: &str, list: &[String], ctx: &Context, findings: &mut Vec<Finding>) {
    if !EGRESS.contains(&name) {
        return;
    }
    let host = if name == "nc" || name == "ncat" || name == "netcat" || name == "telnet" {
        operands(list).first().map(|h| (*h).to_ascii_lowercase())
    } else {
        list.iter().find_map(|a| host_of(a))
    };
    if let Some(h) = &host
        && (ctx.known_hosts.contains(h) || h == "localhost" || h == "127.0.0.1" || h == "::1")
    {
        return;
    }
    let mut f = finding(
        SafetyClass::NetworkEgress,
        "egress",
        format!("{name} {}", host.clone().unwrap_or_default())
            .trim()
            .to_owned(),
        "talks to a host this repo has not talked to before",
    );
    f.host = host;
    findings.push(f);
}
