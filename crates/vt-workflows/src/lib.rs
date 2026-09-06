//! Workflows (12 §B12): parameterised saved commands in Warp's workflow
//! YAML — `name`, `command`, `description`, `tags`, `arguments` with
//! `name`/`description`/`default_value`, `shells` — read from
//! `~/.config/vambiant-term/workflows/`, `<repo>/.vambiant-term/workflows/`
//! and, read-only for drop-in compatibility, `~/.warp/workflows/` and
//! `<repo>/.warp/workflows/`. `{{arg}}` is substituted; the result is a
//! command line to stage, never to run from here.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One argument of a workflow.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Argument {
    /// The `{{name}}` in the command.
    pub name: String,
    /// What it is for.
    #[serde(default)]
    pub description: String,
    /// Prefilled value, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
}

/// One workflow.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Workflow {
    /// Display name.
    pub name: String,
    /// The command with `{{arg}}` placeholders.
    pub command: String,
    /// What it does.
    #[serde(default)]
    pub description: String,
    /// Tags for search.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Arguments in order.
    #[serde(default)]
    pub arguments: Vec<Argument>,
    /// Shells it applies to; empty means all.
    #[serde(default)]
    pub shells: Vec<String>,
    /// Where it was read from.
    #[serde(default)]
    pub source: PathBuf,
    /// The file came from a `.warp/workflows` directory (read-only here).
    #[serde(default)]
    pub warp: bool,
}

/// Why a file was not a workflow.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WorkflowError {
    /// `name` or `command` is missing.
    #[error("{path}: workflow has no `{field}`")]
    Missing {
        /// The file.
        path: PathBuf,
        /// The field.
        field: &'static str,
    },
}

impl Workflow {
    /// Substitutes `{{name}}` with the given values, then the defaults;
    /// a placeholder with neither stays as typed so the user sees it.
    pub fn render(&self, values: &[(String, String)]) -> String {
        let mut out = self.command.clone();
        for arg in &self.arguments {
            let value = values
                .iter()
                .find(|(k, _)| *k == arg.name)
                .map(|(_, v)| v.clone())
                .or_else(|| arg.default_value.clone());
            if let Some(v) = value {
                out = out.replace(&format!("{{{{{}}}}}", arg.name), &v);
            }
        }
        out.trim_end().to_owned()
    }

    /// Placeholders the command uses that no argument declares.
    pub fn undeclared(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut rest = self.command.as_str();
        while let Some(start) = rest.find("{{") {
            let after = &rest[start + 2..];
            let Some(end) = after.find("}}") else { break };
            let name = after[..end].trim();
            if !name.is_empty()
                && !self.arguments.iter().any(|a| a.name == name)
                && !out.iter().any(|n| n == name)
            {
                out.push(name.to_owned());
            }
            rest = &after[end + 2..];
        }
        out
    }
}

/// Reads one workflow file (Warp's YAML shape).
pub fn parse(text: &str, path: &Path) -> Result<Workflow, WorkflowError> {
    let mut wf = Workflow {
        source: path.to_path_buf(),
        warp: path.components().any(|c| c.as_os_str() == ".warp"),
        ..Workflow::default()
    };
    let mut lines = text.lines().peekable();
    while let Some(raw) = lines.next() {
        let line = raw.trim_end();
        if line.trim().is_empty() || line.trim_start().starts_with('#') || line.starts_with(' ') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        match key {
            "name" => wf.name = unquote(value),
            "description" => wf.description = block_or_scalar(value, &mut lines),
            "command" => wf.command = block_or_scalar(value, &mut lines),
            "tags" => wf.tags = list(value, &mut lines),
            "shells" => wf.shells = list(value, &mut lines),
            "arguments" => wf.arguments = arguments(&mut lines),
            _ => {
                // Unknown keys (author, source_url, …) and their nested lines are skipped.
                while lines
                    .peek()
                    .is_some_and(|l| l.starts_with(' ') || l.trim_start().starts_with('-'))
                {
                    lines.next();
                }
            }
        }
    }
    let undeclared = wf.undeclared();
    for name in undeclared {
        wf.arguments.push(Argument {
            name,
            ..Argument::default()
        });
    }
    if wf.name.is_empty() {
        return Err(WorkflowError::Missing {
            path: path.to_path_buf(),
            field: "name",
        });
    }
    if wf.command.is_empty() {
        return Err(WorkflowError::Missing {
            path: path.to_path_buf(),
            field: "command",
        });
    }
    Ok(wf)
}

fn unquote(value: &str) -> String {
    let v = value.trim();
    if (v.starts_with('"') && v.ends_with('"') || v.starts_with('\'') && v.ends_with('\''))
        && v.len() >= 2
    {
        v[1..v.len() - 1].replace("\\\"", "\"")
    } else {
        v.to_owned()
    }
}

/// A scalar, or a `|` / `>` block whose indented lines follow.
fn block_or_scalar<'a>(
    value: &str,
    lines: &mut std::iter::Peekable<impl Iterator<Item = &'a str>>,
) -> String {
    let v = value.trim();
    if v == "|" || v == ">" || v == "|-" || v == ">-" {
        let mut out = Vec::new();
        let mut indent: Option<usize> = None;
        while let Some(next) = lines.peek() {
            if next.trim().is_empty() {
                out.push(String::new());
                lines.next();
                continue;
            }
            let lead = next.len() - next.trim_start().len();
            if lead == 0 {
                break;
            }
            let indent = *indent.get_or_insert(lead);
            out.push(next.get(indent.min(lead)..).unwrap_or("").to_owned());
            lines.next();
        }
        let joined = if v.starts_with('>') {
            out.join(" ")
        } else {
            out.join("\n")
        };
        joined.trim_end().to_owned()
    } else {
        unquote(v)
    }
}

/// `[a, b]` on the line, or `- a` lines below.
fn list<'a>(
    value: &str,
    lines: &mut std::iter::Peekable<impl Iterator<Item = &'a str>>,
) -> Vec<String> {
    let v = value.trim();
    if let Some(inner) = v.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        return inner
            .split(',')
            .map(unquote)
            .filter(|s| !s.is_empty())
            .collect();
    }
    let mut out = Vec::new();
    while let Some(next) = lines.peek() {
        let t = next.trim_start();
        if let Some(item) = t.strip_prefix("- ") {
            out.push(unquote(item));
            lines.next();
        } else {
            break;
        }
    }
    out
}

/// `- name: x` maps, each with its own indented keys.
fn arguments<'a>(lines: &mut std::iter::Peekable<impl Iterator<Item = &'a str>>) -> Vec<Argument> {
    let mut out: Vec<Argument> = Vec::new();
    while let Some(next) = lines.peek() {
        let t = next.trim_start();
        if next.trim().is_empty() {
            lines.next();
            continue;
        }
        if !next.starts_with(' ') && !t.starts_with('-') {
            break;
        }
        let (is_new, body) = match t.strip_prefix("- ") {
            Some(rest) => (true, rest),
            None => (false, t),
        };
        lines.next();
        if is_new {
            out.push(Argument::default());
        }
        let Some(current) = out.last_mut() else {
            continue;
        };
        if let Some((k, v)) = body.split_once(':') {
            let v = unquote(v.trim());
            match k.trim() {
                "name" => current.name = v,
                "description" => current.description = v,
                "default_value" => current.default_value = Some(v),
                _ => {}
            }
        }
    }
    out.into_iter().filter(|a| !a.name.is_empty()).collect()
}

/// Every workflow under the four directories, user first, then repo; a
/// file that does not parse is reported and skipped.
pub fn discover(
    config_dir: &Path,
    home: Option<&Path>,
    repo: Option<&Path>,
) -> (Vec<Workflow>, Vec<String>) {
    let mut dirs = vec![config_dir.join("workflows")];
    if let Some(h) = home {
        dirs.push(h.join(".warp").join("workflows"));
    }
    if let Some(r) = repo {
        dirs.push(r.join(".vambiant-term").join("workflows"));
        dirs.push(r.join(".warp").join("workflows"));
    }
    let mut out = Vec::new();
    let mut problems = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "yaml" || e == "yml"))
            .collect();
        files.sort();
        for path in files {
            match std::fs::read_to_string(&path) {
                Ok(text) => match parse(&text, &path) {
                    Ok(wf) => out.push(wf),
                    Err(e) => problems.push(e.to_string()),
                },
                Err(e) => problems.push(format!("{}: {e}", path.display())),
            }
        }
    }
    (out, problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KILL_PORT: &str = "---\nname: Kill process on port\ncommand: lsof -i tcp:{{port}} | awk 'NR!=1 {print $2}' | xargs kill\ntags:\n  - unix\n  - process\ndescription: Kill a process running on a given port\narguments:\n  - name: port\n    description: The port number\n    default_value: 8080\nsource_url: \"https://example.com\"\nauthor: someone\nshells: [zsh, bash]\n";

    #[test]
    fn parses_warps_shape_and_renders() {
        let wf = parse(KILL_PORT, Path::new("/home/me/.warp/workflows/kill.yaml")).unwrap();
        assert_eq!(wf.name, "Kill process on port");
        assert_eq!(wf.tags, ["unix", "process"]);
        assert_eq!(wf.shells, ["zsh", "bash"]);
        assert_eq!(wf.arguments.len(), 1);
        assert_eq!(wf.arguments[0].default_value.as_deref(), Some("8080"));
        assert!(wf.warp);
        assert_eq!(
            wf.render(&[]),
            "lsof -i tcp:8080 | awk 'NR!=1 {print $2}' | xargs kill"
        );
        assert_eq!(
            wf.render(&[("port".into(), "3000".into())]),
            "lsof -i tcp:3000 | awk 'NR!=1 {print $2}' | xargs kill"
        );
    }

    #[test]
    fn block_commands_and_undeclared_placeholders() {
        let text = "name: deploy\ndescription: |\n  Two lines\n  of text\ncommand: |\n  ssh {{host}} \\\n    'cd {{dir}} && git pull'\n";
        let wf = parse(text, Path::new("deploy.yaml")).unwrap();
        assert_eq!(wf.description, "Two lines\nof text");
        assert_eq!(wf.command, "ssh {{host}} \\\n  'cd {{dir}} && git pull'");
        assert_eq!(
            wf.arguments
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>(),
            ["host", "dir"],
            "placeholders become arguments"
        );
        assert!(!wf.warp);
        assert_eq!(
            wf.render(&[("host".into(), "prod".into())]),
            "ssh prod \\\n  'cd {{dir}} && git pull'",
            "an unset placeholder stays visible"
        );
        assert!(matches!(
            parse("command: ls\n", Path::new("x.yaml")),
            Err(WorkflowError::Missing { field: "name", .. })
        ));
    }

    #[test]
    fn discovery_walks_the_four_directories() {
        let root = std::env::temp_dir().join(format!("vt-workflows-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (cfg, home, repo) = (root.join("cfg"), root.join("home"), root.join("repo"));
        for d in [
            cfg.join("workflows"),
            home.join(".warp/workflows"),
            repo.join(".vambiant-term/workflows"),
            repo.join(".warp/workflows"),
        ] {
            std::fs::create_dir_all(&d).unwrap();
        }
        std::fs::write(cfg.join("workflows/a.yaml"), "name: A\ncommand: echo a\n").unwrap();
        std::fs::write(
            home.join(".warp/workflows/b.yml"),
            "name: B\ncommand: echo b\n",
        )
        .unwrap();
        std::fs::write(
            repo.join(".vambiant-term/workflows/c.yaml"),
            "name: C\ncommand: echo c\n",
        )
        .unwrap();
        std::fs::write(
            repo.join(".warp/workflows/d.yaml"),
            "name: D\ncommand: echo d\n",
        )
        .unwrap();
        std::fs::write(repo.join(".warp/workflows/broken.yaml"), "command: only\n").unwrap();
        std::fs::write(repo.join(".warp/workflows/notes.txt"), "ignored").unwrap();
        let (found, problems) = discover(&cfg, Some(&home), Some(&repo));
        assert_eq!(
            found.iter().map(|w| w.name.as_str()).collect::<Vec<_>>(),
            ["A", "B", "C", "D"]
        );
        assert!(found[1].warp && !found[2].warp && found[3].warp);
        assert_eq!(problems.len(), 1, "{problems:?}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
