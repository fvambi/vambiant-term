//! Command corrections (12 §B8, Warp's "did you mean"): a failed command
//! line, its exit status and output tail, and what the shell can see
//! (executables on `PATH`, entries of the cwd) give a corrected command
//! line. Rule families follow thefuck's (MIT, Vladimir Iakovlev and
//! contributors), rewritten as data here. A correction is a suggestion:
//! it is staged into the editor, never run.

use regex::Regex;

/// A failed command as the corrector sees it.
#[derive(Debug, Clone, Default)]
pub struct Failed<'a> {
    /// The command line as typed.
    pub cmdline: &'a str,
    /// Exit status.
    pub exit: i32,
    /// The last lines of output.
    pub output: &'a str,
    /// Executable names on the session's `PATH`.
    pub executables: &'a [String],
    /// Entries of the working directory.
    pub entries: &'a [String],
}

/// One correction.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Correction {
    /// The corrected command line.
    pub command: String,
    /// Stable rule id (`git-push-upstream`, `misspelled-executable`, …).
    pub rule: String,
    /// One line on why, for the hint.
    pub explanation: String,
}

fn hit(command: String, rule: &str, explanation: impl Into<String>) -> Correction {
    Correction {
        command,
        rule: rule.into(),
        explanation: explanation.into(),
    }
}

/// Words of the command line, quotes kept as typed.
fn words(cmdline: &str) -> Vec<&str> {
    cmdline.split_whitespace().collect()
}

/// Optimal-string-alignment distance: insert, delete, substitute, and a
/// swap of two neighbours each cost one, so `gti` is one away from `git`.
pub fn distance(left: &str, right: &str) -> usize {
    let left: Vec<char> = left.chars().collect();
    let right: Vec<char> = right.chars().collect();
    let (rows, cols) = (left.len(), right.len());
    let mut table = vec![vec![0usize; cols + 1]; rows + 1];
    for (i, row) in table.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in table[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=rows {
        for j in 1..=cols {
            let cost = usize::from(left[i - 1] != right[j - 1]);
            table[i][j] = (table[i - 1][j] + 1)
                .min(table[i][j - 1] + 1)
                .min(table[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && left[i - 1] == right[j - 2] && left[i - 2] == right[j - 1] {
                table[i][j] = table[i][j].min(table[i - 2][j - 2] + 1);
            }
        }
    }
    table[rows][cols]
}

/// The closest candidate within `max` edits, ties broken by length then name.
pub fn closest<'a>(
    word: &str,
    candidates: impl IntoIterator<Item = &'a str>,
    max: usize,
) -> Option<&'a str> {
    let mut best: Option<(usize, &str)> = None;
    for c in candidates {
        if c == word {
            continue;
        }
        let d = distance(word, c);
        if d <= max
            && best.is_none_or(|(bd, bc)| d < bd || (d == bd && (c.len(), c) < (bc.len(), bc)))
        {
            best = Some((d, c));
        }
    }
    best.map(|(_, c)| c)
}

const GIT_SUBCOMMANDS: &[&str] = &[
    "add",
    "bisect",
    "blame",
    "branch",
    "checkout",
    "cherry-pick",
    "clean",
    "clone",
    "commit",
    "diff",
    "fetch",
    "grep",
    "init",
    "log",
    "merge",
    "mv",
    "pull",
    "push",
    "rebase",
    "reflog",
    "remote",
    "reset",
    "restore",
    "revert",
    "rm",
    "show",
    "stash",
    "status",
    "switch",
    "tag",
    "worktree",
];
const CARGO_SUBCOMMANDS: &[&str] = &[
    "add", "bench", "build", "check", "clean", "clippy", "doc", "fmt", "init", "install", "new",
    "publish", "remove", "run", "search", "test", "tree", "update",
];
const DOCKER_SUBCOMMANDS: &[&str] = &[
    "build", "compose", "exec", "images", "logs", "ps", "pull", "push", "rm", "rmi", "run",
    "start", "stop", "volume",
];
const NPM_SUBCOMMANDS: &[&str] = &[
    "audit",
    "ci",
    "init",
    "install",
    "link",
    "ls",
    "outdated",
    "publish",
    "run",
    "start",
    "test",
    "uninstall",
    "update",
];

/// Every correction that applies, most specific first. Empty means none.
pub fn suggest(f: &Failed<'_>) -> Vec<Correction> {
    let mut out = Vec::new();
    let w = words(f.cmdline);
    if w.is_empty() {
        return out;
    }
    let lower = f.output.to_ascii_lowercase();
    named_in_output(f, &w, &mut out);
    permissions(f, &w, &lower, &mut out);
    subcommands(f, &w, &lower, &mut out);
    paths_and_flags(f, &w, &lower, &mut out);
    misspelled_executable(f, &w, &lower, &mut out);
    out.dedup_by(|a, b| a.command == b.command);
    out
}

/// Output that names the fix outright: git's upstream hint, "the most
/// similar command", "Did you mean".
fn named_in_output(f: &Failed<'_>, w: &[&str], out: &mut Vec<Correction>) {
    let output = f.output;
    if let Some(m) = Regex::new(r"git push --set-upstream \S+ \S+")
        .ok()
        .and_then(|re| re.find(output))
    {
        out.push(hit(
            m.as_str().to_owned(),
            "git-push-upstream",
            "the branch has no upstream yet",
        ));
    }
    if let Some(m) = Regex::new(r"(?m)The most similar command is\s*\n?\s*(\S+)")
        .ok()
        .and_then(|re| re.captures(output))
        && let Some(sub) = w.get(1)
    {
        out.push(hit(
            f.cmdline.replacen(sub, &m[1], 1),
            "git-subcommand",
            format!("git suggested `{}`", &m[1]),
        ));
    }
    if let Some(m) = Regex::new(r"Did you mean `?([\w-]+)`?")
        .ok()
        .and_then(|re| re.captures(output))
        && let Some(sub) = w.get(1).filter(|s| **s != &m[1])
    {
        out.push(hit(
            f.cmdline.replacen(sub, &m[1], 1),
            "did-you-mean",
            format!("`{}` was suggested", &m[1]),
        ));
    }
}

/// Denied: `sudo`, or `chmod +x` for a script that is not executable.
fn permissions(f: &Failed<'_>, w: &[&str], lower: &str, out: &mut Vec<Correction>) {
    let denied = [
        "permission denied",
        "operation not permitted",
        "must be root",
        "requires superuser",
        "eacces",
    ];
    if !denied.iter().any(|d| lower.contains(d)) {
        return;
    }
    let first = w[0];
    if (first.starts_with("./") || first.starts_with('/')) && f.exit == 126 {
        out.push(hit(
            format!("chmod +x {first} && {}", f.cmdline),
            "chmod-x",
            "the script is not executable",
        ));
    } else if first != "sudo" {
        out.push(hit(
            format!("sudo {}", f.cmdline),
            "sudo",
            "permission was denied",
        ));
    }
}

/// Subcommand typos for tools that report them without a suggestion.
fn subcommands(f: &Failed<'_>, w: &[&str], lower: &str, out: &mut Vec<Correction>) {
    let Some(sub) = w.get(1) else { return };
    let table: &[(&str, &str, &[&str], &str)] = &[
        (
            "git",
            "is not a git command",
            GIT_SUBCOMMANDS,
            "git-subcommand",
        ),
        (
            "cargo",
            "no such command",
            CARGO_SUBCOMMANDS,
            "cargo-subcommand",
        ),
        (
            "docker",
            "is not a docker command",
            DOCKER_SUBCOMMANDS,
            "docker-subcommand",
        ),
        ("npm", "unknown command", NPM_SUBCOMMANDS, "npm-subcommand"),
    ];
    for (tool, marker, known, rule) in table {
        if w[0] == *tool
            && lower.contains(marker)
            && out
                .iter()
                .all(|c| c.rule != "git-subcommand" && c.rule != "did-you-mean")
            && let Some(best) = closest(sub, known.iter().copied(), 2)
        {
            out.push(hit(
                f.cmdline.replacen(sub, best, 1),
                rule,
                format!("`{sub}` is not a {tool} command"),
            ));
        }
    }
}

/// `cd..`, a directory that nearly matches, `mkdir -p`, `rm -r`, `python3`.
fn paths_and_flags(f: &Failed<'_>, w: &[&str], lower: &str, out: &mut Vec<Correction>) {
    let first = w[0];
    let missing = lower.contains("no such file or directory");
    if first == "cd.." {
        out.push(hit(
            f.cmdline.replacen("cd..", "cd ..", 1),
            "cd-dotdot",
            "a space after cd",
        ));
    }
    if first == "cd"
        && missing
        && let Some(target) = w.get(1)
        && let Some(best) = closest(
            target.trim_end_matches('/'),
            f.entries.iter().map(String::as_str),
            3,
        )
    {
        out.push(hit(
            format!("cd {best}"),
            "cd-fuzzy",
            format!("`{target}` is not here; `{best}` is"),
        ));
    }
    if first == "mkdir" && missing && !w.contains(&"-p") {
        out.push(hit(
            f.cmdline.replacen("mkdir", "mkdir -p", 1),
            "mkdir-p",
            "parent directories are missing",
        ));
    }
    if (first == "rm" || first == "rmdir")
        && lower.contains("is a directory")
        && !w.iter().any(|a| a.starts_with('-') && a.contains('r'))
    {
        out.push(hit(
            f.cmdline.replacen("rm", "rm -r", 1),
            "rm-recursive",
            "the target is a directory",
        ));
    }
    if (first == "python" || first == "pip") && lower.contains("not found") {
        let alt = format!("{first}3");
        if f.executables.contains(&alt) {
            out.push(hit(
                f.cmdline.replacen(first, &alt, 1),
                "python3",
                format!("`{first}` is not installed but `{alt}` is"),
            ));
        }
    }
}

/// A misspelled executable: the closest name on PATH, one edit for short
/// names, two otherwise; only when nothing more specific applied.
fn misspelled_executable(f: &Failed<'_>, w: &[&str], lower: &str, out: &mut Vec<Correction>) {
    let first = w[0];
    let not_found = lower.contains("not found") || f.exit == 127;
    if !not_found || !out.is_empty() {
        return;
    }
    let max = if first.len() <= 3 { 1 } else { 2 };
    if let Some(best) = closest(first, f.executables.iter().map(String::as_str), max) {
        out.push(hit(
            f.cmdline.replacen(first, best, 1),
            "misspelled-executable",
            format!("`{first}` is not on PATH; `{best}` is"),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failed<'a>(
        cmdline: &'a str,
        exit: i32,
        output: &'a str,
        exes: &'a [String],
        entries: &'a [String],
    ) -> Failed<'a> {
        Failed {
            cmdline,
            exit,
            output,
            executables: exes,
            entries,
        }
    }

    #[test]
    fn misspelled_executables_and_subcommands() {
        let exes: Vec<String> = ["git", "grep", "gh", "ls", "python3", "cargo"]
            .map(String::from)
            .to_vec();
        let c = suggest(&failed(
            "gti status",
            127,
            "zsh: command not found: gti",
            &exes,
            &[],
        ));
        assert_eq!(c[0].command, "git status");
        assert_eq!(c[0].rule, "misspelled-executable");
        let c = suggest(&failed(
            "git psuh origin main",
            1,
            "git: 'psuh' is not a git command. See 'git --help'.\n\nThe most similar command is\n\tpush",
            &exes,
            &[],
        ));
        assert_eq!(c[0].command, "git push origin main");
        assert_eq!(c[0].rule, "git-subcommand");
        let c = suggest(&failed(
            "git stauts",
            1,
            "git: 'stauts' is not a git command. See 'git --help'.",
            &exes,
            &[],
        ));
        assert_eq!(c[0].command, "git status");
        let c = suggest(&failed(
            "cargo biuld",
            101,
            "error: no such command: `biuld`\n\n\tDid you mean `build`?",
            &exes,
            &[],
        ));
        assert_eq!(c[0].command, "cargo build");
        assert_eq!(c[0].rule, "did-you-mean");
        let c = suggest(&failed(
            "python x.py",
            127,
            "zsh: command not found: python",
            &exes,
            &[],
        ));
        assert_eq!(c[0].command, "python3 x.py");
        assert!(suggest(&failed("ls", 0, "", &exes, &[])).is_empty());
        assert!(
            suggest(&failed(
                "frobnicate",
                127,
                "command not found: frobnicate",
                &exes,
                &[]
            ))
            .is_empty(),
            "nothing close: nothing"
        );
    }

    #[test]
    fn permissions_paths_and_flags() {
        let c = suggest(&failed(
            "git push",
            1,
            "fatal: The current branch x has no upstream branch.\nTo push the current branch and set the remote as upstream, use\n\n    git push --set-upstream origin x\n",
            &[],
            &[],
        ));
        assert_eq!(c[0].command, "git push --set-upstream origin x");
        let c = suggest(&failed(
            "apt install x",
            100,
            "E: Could not open lock file - open (13: Permission denied)",
            &[],
            &[],
        ));
        assert_eq!(c[0].command, "sudo apt install x");
        let c = suggest(&failed(
            "./deploy.sh",
            126,
            "zsh: permission denied: ./deploy.sh",
            &[],
            &[],
        ));
        assert_eq!(c[0].command, "chmod +x ./deploy.sh && ./deploy.sh");
        let c = suggest(&failed(
            "mkdir a/b/c",
            1,
            "mkdir: a/b: No such file or directory",
            &[],
            &[],
        ));
        assert_eq!(c[0].command, "mkdir -p a/b/c");
        let c = suggest(&failed(
            "rm build",
            1,
            "rm: build: is a directory",
            &[],
            &[],
        ));
        assert_eq!(c[0].command, "rm -r build");
        let entries: Vec<String> = ["Documents", "Downloads", "code"]
            .map(String::from)
            .to_vec();
        let c = suggest(&failed(
            "cd Documnets",
            1,
            "cd: no such file or directory: Documnets",
            &[],
            &entries,
        ));
        assert_eq!(c[0].command, "cd Documents");
        assert_eq!(
            suggest(&failed(
                "cd..",
                127,
                "zsh: command not found: cd..",
                &[],
                &[]
            ))[0]
                .command,
            "cd .."
        );
        assert_eq!(distance("kitten", "sitting"), 3);
        assert_eq!(closest("gti", ["git", "gh", "grep"], 1), Some("git"));
    }
}
