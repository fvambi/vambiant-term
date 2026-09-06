//! A POSIX shell reader for classification (docs/05 §5.1): enough grammar
//! to see every command in `ls && rm -rf /` — through pipes, subshells,
//! groups, `if`/`for`/`while`/`case` bodies, command substitution and the
//! quoting tricks that hide a verb (`r""m`, `\rm`). Nothing is executed
//! or expanded; a word that needs expansion says so. Input this reader
//! cannot follow is an error, never a guess.

mod parser;
mod word;

use std::fmt;

/// One piece of a word after quote removal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    /// Literal text (quotes and backslashes already removed).
    Lit(String),
    /// `$NAME`, `${…}` or `$((…))`: the value is unknown here.
    Var(String),
    /// `$(…)`, `` `…` ``, `<(…)` or `>(…)`: the inner program.
    Subst(Program),
}

/// A shell word.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Word {
    /// Parts in order.
    pub parts: Vec<Part>,
    /// Some part was quoted (relevant for reserved words and evasion notes).
    pub quoted: bool,
    /// Unquoted `*`, `?` or `[` appeared: the word is a pattern.
    pub glob: bool,
}

impl Word {
    /// The text when every part is literal.
    pub fn literal(&self) -> Option<String> {
        let mut out = String::new();
        for p in &self.parts {
            match p {
                Part::Lit(s) => out.push_str(s),
                _ => return None,
            }
        }
        Some(out)
    }

    /// Lossy text for messages: expansions keep their `$` form.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for p in &self.parts {
            match p {
                Part::Lit(s) => out.push_str(s),
                Part::Var(v) => {
                    out.push('$');
                    out.push_str(v);
                }
                Part::Subst(_) => out.push_str("$(…)"),
            }
        }
        out
    }

    /// Any variable or command substitution.
    pub fn has_expansion(&self) -> bool {
        self.parts.iter().any(|p| !matches!(p, Part::Lit(_)))
    }

    /// Programs inside command substitutions of this word.
    pub fn substitutions(&self) -> impl Iterator<Item = &Program> {
        self.parts.iter().filter_map(|p| match p {
            Part::Subst(prog) => Some(prog),
            _ => None,
        })
    }

    fn push_lit(&mut self, c: char) {
        if let Some(Part::Lit(s)) = self.parts.last_mut() {
            s.push(c);
        } else {
            self.parts.push(Part::Lit(c.to_string()));
        }
    }
}

/// Redirection operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)] // the operators themselves
pub enum RedirOp {
    Out,
    Append,
    Clobber,
    In,
    ReadWrite,
    DupOut,
    DupIn,
    Both,
    BothAppend,
    HereDoc,
    HereString,
}

/// One redirection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redirect {
    /// Explicit descriptor (`2>`), if any.
    pub fd: Option<u32>,
    /// The operator.
    pub op: RedirOp,
    /// The file, descriptor or delimiter word.
    pub target: Word,
}

/// `A=b B=c name arg… >out`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Simple {
    /// Leading assignments.
    pub assignments: Vec<(String, Word)>,
    /// Command name and arguments.
    pub words: Vec<Word>,
    /// Redirections anywhere in the command.
    pub redirects: Vec<Redirect>,
}

/// One command of a pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// A simple command.
    Simple(Simple),
    /// `( … )`.
    Subshell(Program),
    /// `{ …; }`.
    Group(Program),
    /// `if`/`while`/`until`/`for`/`case`/function body: every list it
    /// contains, flattened, because each may run.
    Compound {
        /// The reserved word that opened it.
        keyword: String,
        /// Everything inside.
        body: Program,
    },
}

/// `[!] cmd | cmd…`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Pipeline {
    /// Leading `!`.
    pub negated: bool,
    /// Stages in order.
    pub commands: Vec<Command>,
}

/// A list of pipelines. `&&`, `||`, `;`, `&` and newlines all separate;
/// which one does not matter for classification: every member may run.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Program {
    /// Pipelines in order.
    pub pipelines: Vec<Pipeline>,
}

impl Program {
    /// Every simple command in the tree, in order, including those inside
    /// substitutions; `depth` counts substitution nesting.
    pub fn walk<'a>(&'a self, f: &mut dyn FnMut(&'a Simple, &'a Pipeline)) {
        for p in &self.pipelines {
            for c in &p.commands {
                match c {
                    Command::Simple(s) => {
                        f(s, p);
                        for w in s.words.iter().chain(s.assignments.iter().map(|a| &a.1)) {
                            for sub in w.substitutions() {
                                sub.walk(f);
                            }
                        }
                        for r in &s.redirects {
                            for sub in r.target.substitutions() {
                                sub.walk(f);
                            }
                        }
                    }
                    Command::Subshell(prog)
                    | Command::Group(prog)
                    | Command::Compound { body: prog, .. } => {
                        prog.walk(f);
                    }
                }
            }
        }
    }

    /// Every pipeline in the tree, including nested ones.
    pub fn pipelines_deep(&self) -> Vec<&Pipeline> {
        let mut out = Vec::new();
        for p in &self.pipelines {
            out.push(p);
            for c in &p.commands {
                match c {
                    Command::Simple(s) => {
                        for w in &s.words {
                            for sub in w.substitutions() {
                                out.extend(sub.pipelines_deep());
                            }
                        }
                    }
                    Command::Subshell(prog)
                    | Command::Group(prog)
                    | Command::Compound { body: prog, .. } => {
                        out.extend(prog.pipelines_deep());
                    }
                }
            }
        }
        out
    }
}

/// Why the input could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("shell parse failed at byte {at}: {reason}")]
pub struct ParseError {
    /// Byte offset in the input.
    pub at: usize,
    /// What was expected or found.
    pub reason: String,
}

/// Parses a command line.
pub fn parse(text: &str) -> Result<Program, ParseError> {
    let mut p = parser::Parser {
        src: text.chars().collect(),
        pos: 0,
        depth: 0,
        heredocs: Vec::new(),
    };
    let prog = p.list(&[])?;
    p.skip_blank_lines()?;
    if p.pos < p.src.len() {
        return Err(p.err(format!("unexpected `{}`", p.src[p.pos])));
    }
    Ok(prog)
}

const RESERVED_END: &[&str] = &[
    "then", "do", "done", "fi", "else", "elif", "esac", "}", ")", ";;", "in",
];

/// `NAME=value` with an unquoted name.
fn split_assignment(word: &Word) -> Option<(String, Word)> {
    let Some(Part::Lit(first)) = word.parts.first() else {
        return None;
    };
    let eq = first.find('=')?;
    let name = &first[..eq];
    if name.is_empty()
        || !name
            .chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_')
        || !name.chars().all(|c| c.is_alphanumeric() || c == '_')
    {
        return None;
    }
    let mut value = word.clone();
    if let Some(Part::Lit(s)) = value.parts.first_mut() {
        *s = s[eq + 1..].to_owned();
    }
    Some((name.to_owned(), value))
}

impl fmt::Display for Word {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text())
    }
}
