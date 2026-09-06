//! Server-sent events over a blocking reader: `event:` / `data:` lines,
//! blank line terminates one event. Multi-line `data:` joins with `\n`.
//! Unknown fields and comments are ignored, as the spec says.

use std::io::{BufRead, BufReader, Read};

/// One event.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(missing_docs)] // field names are the wire format's own vocabulary
pub struct Event {
    pub name: String,
    pub data: String,
}

/// Iterator over the events of one response body.
#[allow(missing_docs)] // field names are the wire format's own vocabulary
pub struct Events<R: Read> {
    lines: std::io::Lines<BufReader<R>>,
}

impl<R: Read> Events<R> {
    /// Wraps a response body.
    pub fn new(reader: R) -> Self {
        Self {
            lines: BufReader::new(reader).lines(),
        }
    }
}

impl<R: Read> Iterator for Events<R> {
    type Item = std::io::Result<Event>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut name = String::new();
        let mut data: Vec<String> = Vec::new();
        loop {
            match self.lines.next() {
                Some(Ok(line)) => {
                    if line.is_empty() {
                        if data.is_empty() && name.is_empty() {
                            continue;
                        }
                        return Some(Ok(Event {
                            name,
                            data: data.join("\n"),
                        }));
                    }
                    if let Some(rest) = line.strip_prefix("event:") {
                        rest.trim().clone_into(&mut name);
                    } else if let Some(rest) = line.strip_prefix("data:") {
                        data.push(rest.strip_prefix(' ').unwrap_or(rest).to_owned());
                    }
                    // `id:`, `retry:` and `:` comments are ignored.
                }
                Some(Err(e)) => return Some(Err(e)),
                None => {
                    if data.is_empty() {
                        return None;
                    }
                    return Some(Ok(Event {
                        name,
                        data: data.join("\n"),
                    }));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_events_and_joins_data_lines() {
        let body = "event: a\ndata: 1\n\n: comment\ndata: x\ndata: y\n\nid: 9\nevent: b\ndata:{}\n";
        let ev: Vec<_> = Events::new(body.as_bytes()).map(Result::unwrap).collect();
        assert_eq!(
            ev,
            vec![
                Event {
                    name: "a".into(),
                    data: "1".into()
                },
                Event {
                    name: String::new(),
                    data: "x\ny".into()
                },
                Event {
                    name: "b".into(),
                    data: "{}".into()
                },
            ]
        );
    }
}
