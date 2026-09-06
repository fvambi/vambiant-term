//! Words: quoting, escapes, `$` expansions, command substitution and the
//! balanced-delimiter scan that finds where a substitution ends.

use super::parser::Parser;
use super::{ParseError, Part, Program, Word};

impl Parser {
    #[allow(clippy::match_same_arms)] // `<`/`>` must come after the `<(` guard
    pub(super) fn word(&mut self) -> Result<Word, ParseError> {
        let mut w = Word::default();
        let start = self.pos;
        while let Some(c) = self.peek() {
            match c {
                ' ' | '\t' | '\n' | '|' | '&' | ';' | ')' => break,
                '(' if self.pos > start => break,
                '<' | '>' if self.peek_at(1) == Some('(') => {
                    self.pos += 2;
                    let inner = self.balanced(')')?;
                    w.parts
                        .push(Part::Subst(parse_inner(&inner, self.pos, self.depth)?));
                }
                '<' | '>' => break,
                '\\' => {
                    self.pos += 1;
                    match self.peek() {
                        Some('\n') => self.pos += 1,
                        Some(e) => {
                            w.push_lit(e);
                            w.quoted = true;
                            self.pos += 1;
                        }
                        None => return Err(self.err("dangling backslash")),
                    }
                }
                '\'' => {
                    self.pos += 1;
                    let s = self.until_quote('\'')?;
                    w.quoted = true;
                    if s.is_empty() {
                        w.parts.push(Part::Lit(String::new()));
                    }
                    for ch in s.chars() {
                        w.push_lit(ch);
                    }
                }
                '"' => {
                    self.pos += 1;
                    w.quoted = true;
                    self.double_quoted(&mut w)?;
                }
                '$' => self.dollar(&mut w)?,
                '`' => {
                    self.pos += 1;
                    let inner = self.until_quote('`')?;
                    w.parts
                        .push(Part::Subst(parse_inner(&inner, self.pos, self.depth)?));
                }
                '*' | '?' | '[' => {
                    w.glob = true;
                    w.push_lit(c);
                    self.pos += 1;
                }
                _ => {
                    w.push_lit(c);
                    self.pos += 1;
                }
            }
        }
        if self.pos == start {
            return Err(self.err("expected a word"));
        }
        Ok(w)
    }

    pub(super) fn until_quote(&mut self, q: char) -> Result<String, ParseError> {
        let start = self.pos;
        let mut out = String::new();
        loop {
            match self.peek() {
                None => {
                    self.pos = start;
                    return Err(self.err(format!("unterminated {q} quote")));
                }
                Some(c) if c == q => {
                    self.pos += 1;
                    return Ok(out);
                }
                Some('\\') if q == '`' && matches!(self.peek_at(1), Some('`' | '\\' | '$')) => {
                    out.push(self.src[self.pos + 1]);
                    self.pos += 2;
                }
                Some(c) => {
                    out.push(c);
                    self.pos += 1;
                }
            }
        }
    }

    pub(super) fn double_quoted(&mut self, w: &mut Word) -> Result<(), ParseError> {
        let start = self.pos;
        let mut any = false;
        loop {
            match self.peek() {
                None => {
                    self.pos = start;
                    return Err(self.err("unterminated \" quote"));
                }
                Some('"') => {
                    self.pos += 1;
                    if !any {
                        w.parts.push(Part::Lit(String::new()));
                    }
                    return Ok(());
                }
                Some('\\') => {
                    match self.peek_at(1) {
                        Some(e @ ('$' | '"' | '\\' | '`')) => {
                            w.push_lit(e);
                            self.pos += 2;
                        }
                        Some('\n') => self.pos += 2,
                        _ => {
                            w.push_lit('\\');
                            self.pos += 1;
                        }
                    }
                    any = true;
                }
                Some('$') => {
                    self.dollar(w)?;
                    any = true;
                }
                Some('`') => {
                    self.pos += 1;
                    let inner = self.until_quote('`')?;
                    w.parts
                        .push(Part::Subst(parse_inner(&inner, self.pos, self.depth)?));
                    any = true;
                }
                Some(c) => {
                    w.push_lit(c);
                    self.pos += 1;
                    any = true;
                }
            }
        }
    }

    pub(super) fn dollar(&mut self, w: &mut Word) -> Result<(), ParseError> {
        self.pos += 1;
        match self.peek() {
            Some('(') if self.peek_at(1) == Some('(') => {
                self.pos += 2;
                let inner = self.balanced(')')?;
                if self.peek() != Some(')') {
                    return Err(self.err("expected `))`"));
                }
                self.pos += 1;
                w.parts.push(Part::Var(format!("(({inner}))")));
            }
            Some('(') => {
                self.pos += 1;
                let inner = self.balanced(')')?;
                w.parts
                    .push(Part::Subst(parse_inner(&inner, self.pos, self.depth)?));
            }
            Some('{') => {
                self.pos += 1;
                let inner = self.balanced('}')?;
                w.parts.push(Part::Var(inner));
            }
            Some(c) if c.is_alphanumeric() || c == '_' => {
                let mut name = String::new();
                while let Some(c) = self.peek() {
                    if c.is_alphanumeric() || c == '_' {
                        name.push(c);
                        self.pos += 1;
                    } else {
                        break;
                    }
                    if name.len() == 1 && name.chars().all(|c| c.is_ascii_digit()) {
                        break;
                    }
                }
                w.parts.push(Part::Var(name));
            }
            Some(c @ ('@' | '*' | '#' | '?' | '-' | '$' | '!')) => {
                self.pos += 1;
                w.parts.push(Part::Var(c.to_string()));
            }
            _ => w.push_lit('$'),
        }
        Ok(())
    }

    /// Text up to the matching `close`, honouring nesting and quotes.
    pub(super) fn balanced(&mut self, close: char) -> Result<String, ParseError> {
        let open = match close {
            ')' => '(',
            '}' => '{',
            _ => close,
        };
        let start = self.pos;
        let mut depth = 0usize;
        let mut out = String::new();
        loop {
            let Some(c) = self.peek() else {
                self.pos = start;
                return Err(self.err(format!("expected `{close}`")));
            };
            match c {
                '\\' => {
                    out.push(c);
                    if let Some(n) = self.peek_at(1) {
                        out.push(n);
                    }
                    self.pos += 2;
                }
                '\'' | '"' => {
                    let q = c;
                    out.push(q);
                    self.pos += 1;
                    loop {
                        match self.peek() {
                            None => {
                                self.pos = start;
                                return Err(self.err(format!("unterminated {q} quote")));
                            }
                            Some('\\') if q == '"' => {
                                out.push('\\');
                                if let Some(n) = self.peek_at(1) {
                                    out.push(n);
                                }
                                self.pos += 2;
                            }
                            Some(e) if e == q => {
                                out.push(e);
                                self.pos += 1;
                                break;
                            }
                            Some(e) => {
                                out.push(e);
                                self.pos += 1;
                            }
                        }
                    }
                }
                c if c == open => {
                    depth += 1;
                    out.push(c);
                    self.pos += 1;
                }
                c if c == close => {
                    if depth == 0 {
                        self.pos += 1;
                        return Ok(out);
                    }
                    depth -= 1;
                    out.push(c);
                    self.pos += 1;
                }
                _ => {
                    out.push(c);
                    self.pos += 1;
                }
            }
        }
    }
}

pub(super) fn parse_inner(text: &str, at: usize, depth: usize) -> Result<Program, ParseError> {
    if depth > 32 {
        return Err(ParseError {
            at,
            reason: "substitutions nested too deep".into(),
        });
    }
    let mut p = Parser {
        src: text.chars().collect(),
        pos: 0,
        depth: depth + 1,
        heredocs: Vec::new(),
    };
    let prog = p.list(&[])?;
    p.skip_blank_lines()?;
    if p.pos < p.src.len() {
        return Err(ParseError {
            at,
            reason: format!("unexpected `{}` inside a substitution", p.src[p.pos]),
        });
    }
    Ok(prog)
}
