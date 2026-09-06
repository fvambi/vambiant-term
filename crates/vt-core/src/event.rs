//! Out-of-band terminal events: things the byte stream asked the *host* to
//! do rather than the grid.
//!
//! Policy is decided upstream (`vt-config` `[terminal.osc]`): the core reports
//! a clipboard write, it never touches the clipboard.

/// Clipboard selection an OSC 52 write targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipboardTarget {
    /// The system clipboard (`c`).
    Clipboard,
    /// The primary selection (`p`/`s`), mapped to the clipboard on macOS.
    Selection,
}

/// Events emitted by [`TerminalCore::take_events`](crate::TerminalCore::take_events).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TermEvent {
    /// BEL received.
    Bell,
    /// Window title changed (OSC 0/2).
    Title(String),
    /// Working directory reported (OSC 7, OSC 9;9, OSC 1337 `CurrentDir`).
    Pwd(String),
    /// A program asked to write to the clipboard (OSC 52). Bytes are the
    /// decoded payload; `vt-redact` and policy see it before anything else.
    ClipboardWrite {
        /// Which selection.
        target: ClipboardTarget,
        /// Decoded contents.
        contents: Vec<u8>,
    },
    /// A shell-integration mark (OSC 133 / 633) and the absolute row —
    /// scrollback rows plus cursor row — the cursor was on right after it.
    ShellMark {
        /// The mark.
        mark: crate::osc::ShellMark,
        /// Absolute row from the top of scrollback.
        row: u64,
    },
}

/// Decode an OSC 7 / OSC 1337 `CurrentDir` value into a filesystem path.
///
/// Accepts `file://HOST/path`, `file:///path`, and a bare `/path`. Hostnames
/// are inconsistent in the wild (empty, `localhost`, unknown machines), so the
/// host is ignored; percent-encoding is decoded leniently — a malformed escape
/// is kept verbatim rather than rejected (docs/10 §10).
pub fn pwd_from_osc7(value: &str) -> String {
    let rest = match value.strip_prefix("file://") {
        Some(r) => match r.find('/') {
            Some(i) => &r[i..],
            None => "/",
        },
        None => value,
    };
    percent_decode(rest)
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = &s[i + 1..(i + 3).min(s.len())];
            if hex.len() == 2
                && let Ok(v) = u8::from_str_radix(hex, 16)
            {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::pwd_from_osc7;

    #[test]
    fn decodes_file_urls_leniently() {
        assert_eq!(pwd_from_osc7("file://localhost/tmp/x"), "/tmp/x");
        assert_eq!(pwd_from_osc7("file:///tmp/x"), "/tmp/x");
        assert_eq!(pwd_from_osc7("file://some.host/a%20b/c"), "/a b/c");
        assert_eq!(pwd_from_osc7("/plain/path"), "/plain/path");
        assert_eq!(pwd_from_osc7("file://host"), "/");
        assert_eq!(pwd_from_osc7("/bad%2"), "/bad%2");
        assert_eq!(pwd_from_osc7("/bad%zz"), "/bad%zz");
    }
}
