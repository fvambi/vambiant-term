//! Deleting, overwriting and history-rewriting commands, with the
//! worktree test the floor relies on.

use super::{basename, finding, has_flag, operands};
use crate::classify::{Context, Finding, SafetyClass, inside_worktree, resolve};
use crate::paths::{is_protected, is_system_path};
use crate::shell::{RedirOp, Simple};

pub(super) fn remove(name: &str, list: &[String], ctx: &Context, findings: &mut Vec<Finding>) {
    let recursive =
        has_flag(list, "--recursive", Some('r')) || has_flag(list, "--recursive", Some('R'));
    let targets = operands(list);
    let rule = if recursive { "rm-recursive" } else { "rm" };
    let detail = if recursive {
        "deletes directories and everything under them"
    } else {
        "deletes files"
    };
    destructive_targets(name, rule, &targets, ctx, findings, detail);
}

pub(super) fn destructive_targets(
    name: &str,
    rule: &str,
    targets: &[&str],
    ctx: &Context,
    findings: &mut Vec<Finding>,
    detail: &str,
) {
    if targets.is_empty() {
        let mut f = finding(SafetyClass::Destructive, rule, name, detail);
        f.outside_worktree = true;
        findings.push(f);
        return;
    }
    for t in targets {
        let mut f = finding(SafetyClass::Destructive, rule, *t, detail);
        f.outside_worktree = !target_inside(t, ctx);
        findings.push(f);
    }
}

/// A destructive target counts as inside only when it is literal (its
/// glob prefix, if any) and resolves under the worktree.
pub(super) fn target_inside(target: &str, ctx: &Context) -> bool {
    if target.contains('$') || target.contains("$(") {
        return false;
    }
    let prefix = target.split(['*', '?', '[']).next().unwrap_or("");
    let prefix = if target.contains(['*', '?', '[']) {
        match prefix.rfind('/') {
            Some(i) => &prefix[..=i],
            None => ".",
        }
    } else {
        target
    };
    inside_worktree(&resolve(prefix, ctx), ctx)
}

pub(super) fn git(list: &[String], ctx: &Context, findings: &mut Vec<Finding>) {
    let ops = operands(list);
    let Some(sub) = ops.first() else { return };
    match *sub {
        "reset" if has_flag(list, "--hard", None) => inside_destructive(
            "git reset --hard",
            "git-reset-hard",
            ctx,
            findings,
            "discards uncommitted changes",
        ),
        "clean"
            if list
                .iter()
                .any(|a| a.starts_with('-') && !a.starts_with("--") && a.contains('f'))
                || has_flag(list, "--force", None) =>
        {
            inside_destructive(
                "git clean",
                "git-clean",
                ctx,
                findings,
                "deletes untracked files",
            );
        }
        "checkout" if list.iter().any(|a| a == "--" || a == ".") => inside_destructive(
            "git checkout --",
            "git-checkout-discard",
            ctx,
            findings,
            "discards changes in the working tree",
        ),
        "restore" if !has_flag(list, "--staged", Some('S')) && ops.len() > 1 => inside_destructive(
            "git restore",
            "git-restore",
            ctx,
            findings,
            "discards changes in the working tree",
        ),
        "branch"
            if has_flag(list, "--delete", Some('D'))
                && (has_flag(list, "--force", Some('D')) || list.iter().any(|a| a == "-D")) =>
        {
            inside_destructive(
                "git branch -D",
                "git-branch-delete",
                ctx,
                findings,
                "deletes a branch, merged or not",
            );
        }
        "stash" if matches!(ops.get(1).copied(), Some("drop" | "clear")) => inside_destructive(
            "git stash drop",
            "git-stash-drop",
            ctx,
            findings,
            "throws away stashed changes",
        ),
        "filter-branch" | "filter-repo" => inside_destructive(
            &format!("git {sub}"),
            "git-rewrite",
            ctx,
            findings,
            "rewrites history",
        ),
        "push" => git_push(list, &ops, ctx, findings),
        _ => {}
    }
}

pub(super) fn git_push(list: &[String], ops: &[&str], ctx: &Context, findings: &mut Vec<Finding>) {
    let force = has_flag(list, "--force", Some('f'))
        || list
            .iter()
            .any(|a| a.starts_with("--force-with-lease") || a == "--force-if-includes");
    let delete =
        has_flag(list, "--delete", Some('d')) || ops.iter().skip(2).any(|r| r.starts_with(':'));
    let plus = ops.iter().skip(2).any(|r| r.starts_with('+'));
    if force || delete || plus {
        let refspec = ops.get(2).copied();
        let branch = refspec.map(|r| {
            r.trim_start_matches('+')
                .rsplit(':')
                .next()
                .unwrap_or(r)
                .to_owned()
        });
        let mut f = finding(
            SafetyClass::IrreversibleRemote,
            if delete {
                "git-push-delete"
            } else {
                "git-push-force"
            },
            format!("git push {}", list.join(" ")).trim().to_owned(),
            "rewrites or removes history on the remote",
        );
        f.protected_branch = branch
            .as_deref()
            .is_none_or(|b| b == "HEAD" || is_protected(b, &ctx.protected_branches));
        if let Some(b) = branch {
            f.token = format!("git push … {b}");
        }
        findings.push(f);
    }
}

pub(super) fn inside_destructive(
    token: &str,
    rule: &str,
    ctx: &Context,
    findings: &mut Vec<Finding>,
    detail: &str,
) {
    let mut f = finding(SafetyClass::Destructive, rule, token, detail);
    f.outside_worktree = !inside_worktree(&resolve(".", ctx), ctx);
    findings.push(f);
}

pub(super) fn find(list: &[String], ctx: &Context, findings: &mut Vec<Finding>) {
    let deletes = list.iter().any(|a| a == "-delete")
        || list
            .iter()
            .zip(list.iter().skip(1))
            .any(|(a, b)| (a == "-exec" || a == "-execdir" || a == "-ok") && basename(b) == "rm");
    if deletes {
        let start: Vec<&str> = list
            .iter()
            .take_while(|a| !a.starts_with('-'))
            .map(String::as_str)
            .collect();
        let targets = if start.is_empty() { vec!["."] } else { start };
        destructive_targets(
            "find",
            "find-delete",
            &targets,
            ctx,
            findings,
            "deletes every file that matches",
        );
    }
}

pub(super) fn sql(list: &[String], findings: &mut Vec<Finding>) {
    for a in list {
        let upper = a.to_ascii_uppercase();
        let words: Vec<&str> = upper
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .filter(|w| !w.is_empty())
            .collect();
        let hit = words.windows(2).any(|w| {
            matches!(
                w,
                ["DROP", "TABLE" | "DATABASE" | "SCHEMA"]
                    | ["TRUNCATE", "TABLE"]
                    | ["DELETE", "FROM"]
            )
        }) || words.iter().any(|w| *w == "FLUSHALL" || *w == "FLUSHDB");
        if hit {
            let mut f = finding(
                SafetyClass::Destructive,
                "sql-destructive",
                a.chars().take(60).collect::<String>(),
                "drops or empties data in a database",
            );
            f.outside_worktree = true;
            findings.push(f);
        }
    }
}

pub(super) fn redirects(cmd: &Simple, ctx: &Context, findings: &mut Vec<Finding>) {
    for r in &cmd.redirects {
        if !matches!(
            r.op,
            RedirOp::Out | RedirOp::Clobber | RedirOp::Both | RedirOp::Append | RedirOp::BothAppend
        ) {
            continue;
        }
        let Some(target) = r.target.literal() else {
            continue;
        };
        let path = resolve(&target, ctx);
        if target.starts_with("/dev/")
            && (target.contains("disk") || target.contains("/sd") || target.contains("nvme"))
        {
            let mut f = finding(
                SafetyClass::Destructive,
                "write-device",
                target.clone(),
                "writes raw bytes to a disk device",
            );
            f.outside_worktree = true;
            findings.push(f);
        } else if is_system_path(&path) {
            findings.push(finding(
                SafetyClass::Privilege,
                "write-system-path",
                target,
                "writes into a system directory",
            ));
        }
    }
}
