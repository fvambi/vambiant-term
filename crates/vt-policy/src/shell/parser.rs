//! Lists, pipelines, compound commands, simple commands and redirections.
//! Words and quoting live in `word.rs`.

use super::{
    Command, ParseError, Pipeline, Program, RESERVED_END, RedirOp, Redirect, Simple,
    split_assignment,
};

pub(super) struct Parser {
    pub(super) src: Vec<char>,
    pub(super) pos: usize,
    pub(super) depth: usize,
    pub(super) heredocs: Vec<(String, bool)>,
}

impl Parser {
    pub(super) fn err(&self, reason: impl Into<String>) -> ParseError {
        ParseError {
            at: self.pos,
            reason: reason.into(),
        }
    }

    pub(super) fn peek(&self) -> Option<char> {
        self.src.get(self.pos).copied()
    }

    pub(super) fn peek_at(&self, n: usize) -> Option<char> {
        self.src.get(self.pos + n).copied()
    }

    pub(super) fn starts_with(&self, s: &str) -> bool {
        s.chars()
            .enumerate()
            .all(|(i, c)| self.peek_at(i) == Some(c))
    }

    pub(super) fn skip_spaces(&mut self) -> Result<(), ParseError> {
        loop {
            match self.peek() {
                Some(' ' | '\t') => self.pos += 1,
                Some('\\') if self.peek_at(1) == Some('\n') => self.pos += 2,
                Some('#') => {
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.pos += 1;
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    /// Newlines end commands; pending here-doc bodies follow the first one.
    pub(super) fn newline(&mut self) -> Result<(), ParseError> {
        self.pos += 1;
        let pending = std::mem::take(&mut self.heredocs);
        for (delim, strip_tabs) in pending {
            loop {
                if self.pos >= self.src.len() {
                    return Err(self.err(format!("here-document `{delim}` is never closed")));
                }
                let start = self.pos;
                while let Some(c) = self.peek() {
                    if c == '\n' {
                        break;
                    }
                    self.pos += 1;
                }
                let line: String = self.src[start..self.pos].iter().collect();
                if self.peek() == Some('\n') {
                    self.pos += 1;
                }
                let line = if strip_tabs {
                    line.trim_start_matches('\t')
                } else {
                    line.as_str()
                };
                if line == delim {
                    break;
                }
            }
        }
        Ok(())
    }

    pub(super) fn skip_blank_lines(&mut self) -> Result<(), ParseError> {
        loop {
            self.skip_spaces()?;
            if self.peek() == Some('\n') {
                self.newline()?;
            } else {
                return Ok(());
            }
        }
    }

    pub(super) fn at_operator(&self) -> Option<&'static str> {
        ["&&", "||", "|&", ";;", ";&", ";", "|", "&", "(", ")"]
            .into_iter()
            .find(|op| self.starts_with(op))
    }

    /// A list until one of `until` (reserved words) or a closing operator.
    pub(super) fn list(&mut self, until: &[&str]) -> Result<Program, ParseError> {
        let mut prog = Program::default();
        loop {
            self.skip_blank_lines()?;
            if self.pos >= self.src.len() || matches!(self.at_operator(), Some(")" | ";;" | ";&")) {
                return Ok(prog);
            }
            if self
                .peek_reserved()
                .is_some_and(|w| until.contains(&w.as_str()) || RESERVED_END.contains(&w.as_str()))
            {
                return Ok(prog);
            }
            prog.pipelines.push(self.pipeline()?);
            self.skip_spaces()?;
            match self.at_operator() {
                Some(op @ ("&&" | "||" | ";" | "&")) => self.pos += op.len(),
                Some(")" | ";;" | ";&") | None => {
                    if self.peek() == Some('\n') {
                        self.newline()?;
                    } else if self.pos < self.src.len() && self.at_operator().is_none() {
                        return Err(self.err("expected a separator"));
                    }
                }
                Some(op) => return Err(self.err(format!("unexpected `{op}`"))),
            }
        }
    }

    pub(super) fn pipeline(&mut self) -> Result<Pipeline, ParseError> {
        let mut pipe = Pipeline::default();
        self.skip_spaces()?;
        if self.peek_reserved().as_deref() == Some("!") {
            pipe.negated = true;
            self.pos += 1;
        }
        loop {
            self.skip_spaces()?;
            pipe.commands.push(self.command()?);
            self.skip_spaces()?;
            match self.at_operator() {
                Some("|&") => self.pos += 2,
                Some("|") => self.pos += 1,
                _ => return Ok(pipe),
            }
            self.skip_blank_lines()?;
        }
    }

    /// The unquoted literal word at the cursor, without consuming it.
    pub(super) fn peek_reserved(&self) -> Option<String> {
        let mut i = self.pos;
        let mut out = String::new();
        while let Some(&c) = self.src.get(i) {
            if c.is_whitespace() || "|&;()<>\"'`$\\".contains(c) {
                break;
            }
            out.push(c);
            i += 1;
        }
        if out.is_empty() {
            return None;
        }
        if out == "{"
            || out == "}"
            || out == "!"
            || matches!(self.src.get(i), Some(c) if "\"'`$\\".contains(*c))
        {
            return if out.len() == 1 && ["{", "}", "!"].contains(&out.as_str()) {
                Some(out)
            } else {
                None
            };
        }
        Some(out)
    }

    pub(super) fn command(&mut self) -> Result<Command, ParseError> {
        if self.peek() == Some('(') {
            self.pos += 1;
            self.depth += 1;
            let body = self.list(&[])?;
            self.skip_blank_lines()?;
            if self.peek() != Some(')') {
                return Err(self.err("expected `)`"));
            }
            self.pos += 1;
            self.depth -= 1;
            self.trailing_redirects()?;
            return Ok(Command::Subshell(body));
        }
        let word = self.peek_reserved();
        match word.as_deref() {
            Some("{") => {
                self.pos += 1;
                let body = self.list(&["}"])?;
                self.expect_word("}")?;
                self.trailing_redirects()?;
                Ok(Command::Group(body))
            }
            Some("if") => self.compound_if(),
            Some(kw @ ("while" | "until")) => {
                let kw = kw.to_owned();
                self.pos += kw.len();
                let mut body = self.list(&["do"])?;
                self.expect_word("do")?;
                body.pipelines.extend(self.list(&["done"])?.pipelines);
                self.expect_word("done")?;
                self.trailing_redirects()?;
                Ok(Command::Compound { keyword: kw, body })
            }
            Some("for") => self.compound_for(),
            Some("case") => self.compound_case(),
            Some("function") => {
                self.pos += "function".len();
                self.skip_spaces()?;
                let _name = self.word()?;
                self.skip_blank_lines()?;
                let body = self.command()?;
                Ok(Command::Compound {
                    keyword: "function".into(),
                    body: Program {
                        pipelines: vec![Pipeline {
                            negated: false,
                            commands: vec![body],
                        }],
                    },
                })
            }
            _ => self.simple(),
        }
    }

    pub(super) fn expect_word(&mut self, w: &str) -> Result<(), ParseError> {
        self.skip_blank_lines()?;
        if self.peek_reserved().as_deref() == Some(w) {
            self.pos += w.len();
            Ok(())
        } else {
            Err(self.err(format!("expected `{w}`")))
        }
    }

    pub(super) fn compound_if(&mut self) -> Result<Command, ParseError> {
        self.pos += 2;
        let mut body = self.list(&["then"])?;
        self.expect_word("then")?;
        body.pipelines
            .extend(self.list(&["elif", "else", "fi"])?.pipelines);
        loop {
            self.skip_blank_lines()?;
            match self.peek_reserved().as_deref() {
                Some("elif") => {
                    self.pos += 4;
                    body.pipelines.extend(self.list(&["then"])?.pipelines);
                    self.expect_word("then")?;
                    body.pipelines
                        .extend(self.list(&["elif", "else", "fi"])?.pipelines);
                }
                Some("else") => {
                    self.pos += 4;
                    body.pipelines.extend(self.list(&["fi"])?.pipelines);
                }
                Some("fi") => {
                    self.pos += 2;
                    self.trailing_redirects()?;
                    return Ok(Command::Compound {
                        keyword: "if".into(),
                        body,
                    });
                }
                _ => return Err(self.err("expected `fi`")),
            }
        }
    }

    pub(super) fn compound_for(&mut self) -> Result<Command, ParseError> {
        self.pos += 3;
        self.skip_spaces()?;
        let _name = self.word()?;
        self.skip_blank_lines()?;
        let mut body = Program::default();
        if self.peek_reserved().as_deref() == Some("in") {
            self.pos += 2;
            loop {
                self.skip_spaces()?;
                if matches!(self.peek(), Some(';' | '\n') | None) {
                    break;
                }
                let w = self.word()?;
                // Words of the list may carry substitutions that run.
                for sub in w.substitutions() {
                    body.pipelines.extend(sub.pipelines.iter().cloned());
                }
            }
            if self.peek() == Some(';') {
                self.pos += 1;
            }
        } else if self.peek() == Some(';') {
            self.pos += 1;
        }
        self.expect_word("do")?;
        body.pipelines.extend(self.list(&["done"])?.pipelines);
        self.expect_word("done")?;
        self.trailing_redirects()?;
        Ok(Command::Compound {
            keyword: "for".into(),
            body,
        })
    }

    pub(super) fn compound_case(&mut self) -> Result<Command, ParseError> {
        self.pos += 4;
        self.skip_spaces()?;
        let _subject = self.word()?;
        self.expect_word("in")?;
        let mut body = Program::default();
        loop {
            self.skip_blank_lines()?;
            if self.peek_reserved().as_deref() == Some("esac") {
                self.pos += 4;
                self.trailing_redirects()?;
                return Ok(Command::Compound {
                    keyword: "case".into(),
                    body,
                });
            }
            if self.peek() == Some('(') {
                self.pos += 1;
            }
            loop {
                self.skip_spaces()?;
                match self.peek() {
                    Some(')') => {
                        self.pos += 1;
                        break;
                    }
                    Some('|') => self.pos += 1,
                    None => return Err(self.err("expected `)` after a case pattern")),
                    _ => {
                        self.word()?;
                    }
                }
            }
            body.pipelines.extend(self.list(&["esac"])?.pipelines);
            self.skip_blank_lines()?;
            if self.starts_with(";;") || self.starts_with(";&") {
                self.pos += 2;
            }
        }
    }

    pub(super) fn trailing_redirects(&mut self) -> Result<(), ParseError> {
        loop {
            self.skip_spaces()?;
            if self.redirect()?.is_none() {
                return Ok(());
            }
        }
    }

    pub(super) fn simple(&mut self) -> Result<Command, ParseError> {
        let mut cmd = Simple::default();
        loop {
            self.skip_spaces()?;
            match self.peek() {
                None | Some('\n') => break,
                Some(c) if self.at_operator().is_some() && c != '(' => break,
                Some('(') => {
                    if cmd.words.len() == 1
                        && cmd.assignments.is_empty()
                        && self.peek_at(1) == Some(')')
                    {
                        // `name() body`: a function definition; its body may run later.
                        self.pos += 2;
                        self.skip_blank_lines()?;
                        let body = self.command()?;
                        return Ok(Command::Compound {
                            keyword: "function".into(),
                            body: Program {
                                pipelines: vec![Pipeline {
                                    negated: false,
                                    commands: vec![body],
                                }],
                            },
                        });
                    }
                    return Err(self.err("unexpected `(`"));
                }
                _ => {}
            }
            if let Some(r) = self.redirect()? {
                cmd.redirects.push(r);
                continue;
            }
            if cmd.words.is_empty()
                && let Some(w) = self.peek_reserved()
                && RESERVED_END.contains(&w.as_str())
            {
                if cmd.assignments.is_empty() {
                    return Err(self.err(format!("unexpected `{w}`")));
                }
                break;
            }
            let word = self.word()?;
            if cmd.words.is_empty()
                && let Some((name, value)) = split_assignment(&word)
            {
                cmd.assignments.push((name, value));
                continue;
            }
            cmd.words.push(word);
        }
        if cmd.words.is_empty() && cmd.assignments.is_empty() && cmd.redirects.is_empty() {
            return Err(self.err("expected a command"));
        }
        Ok(Command::Simple(cmd))
    }

    pub(super) fn redirect(&mut self) -> Result<Option<Redirect>, ParseError> {
        let save = self.pos;
        let mut fd = None;
        let mut digits = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                digits.push(c);
                self.pos += 1;
            } else {
                break;
            }
        }
        if !digits.is_empty() {
            if matches!(self.peek(), Some('<' | '>')) {
                fd = digits.parse().ok();
            } else {
                self.pos = save;
                return Ok(None);
            }
        }
        let ops: &[(&str, RedirOp)] = &[
            ("&>>", RedirOp::BothAppend),
            ("&>", RedirOp::Both),
            (">>", RedirOp::Append),
            (">|", RedirOp::Clobber),
            (">&", RedirOp::DupOut),
            ("<<<", RedirOp::HereString),
            ("<<-", RedirOp::HereDoc),
            ("<<", RedirOp::HereDoc),
            ("<&", RedirOp::DupIn),
            ("<>", RedirOp::ReadWrite),
            (">", RedirOp::Out),
            ("<", RedirOp::In),
        ];
        for (text, op) in ops {
            if self.starts_with(text) {
                if (*text == "<" || *text == ">") && self.peek_at(1) == Some('(') {
                    self.pos = save;
                    return Ok(None); // process substitution is a word
                }
                let strip_tabs = *text == "<<-";
                self.pos += text.len();
                self.skip_spaces()?;
                let target = self.word()?;
                if *op == RedirOp::HereDoc {
                    let delim = target.literal().unwrap_or_else(|| target.text());
                    self.heredocs.push((delim, strip_tabs));
                }
                return Ok(Some(Redirect {
                    fd,
                    op: *op,
                    target,
                }));
            }
        }
        self.pos = save;
        Ok(None)
    }
}
