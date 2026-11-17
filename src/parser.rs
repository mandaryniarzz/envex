use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quote {
    None,
    Single,
    Double,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub key: String,

    pub value: String,
    pub quote: Quote,
    pub export: bool,

    pub comment: Option<String>,

    pub line: usize,

    pub raw: String,
}

#[derive(Debug, Clone)]
pub enum Line {
    Blank,
    Comment(String),
    Entry(Entry),
    Invalid {
        line: usize,
        raw: String,
        reason: String,
    },
}

#[derive(Debug, Clone, Default)]
pub struct EnvFile {
    pub lines: Vec<Line>,
}

impl EnvFile {
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.lines.iter().filter_map(|l| match l {
            Line::Entry(e) => Some(e),
            _ => None,
        })
    }

    pub fn get(&self, key: &str) -> Option<&Entry> {
        self.entries().find(|e| e.key == key)
    }

    pub fn keys(&self) -> Vec<String> {
        let mut seen = HashSet::new();
        self.entries()
            .filter(|e| seen.insert(e.key.clone()))
            .map(|e| e.key.clone())
            .collect()
    }

    pub fn key_set(&self) -> HashSet<String> {
        self.entries().map(|e| e.key.clone()).collect()
    }

    pub fn invalid(&self) -> impl Iterator<Item = (usize, &str, &str)> {
        self.lines.iter().filter_map(|l| match l {
            Line::Invalid { line, raw, reason } => Some((*line, raw.as_str(), reason.as_str())),
            _ => None,
        })
    }

    pub fn attached_comments(&self, line_index: usize) -> Vec<String> {
        let mut out = Vec::new();
        let mut i = line_index;
        while i > 0 {
            match &self.lines[i - 1] {
                Line::Comment(c) => {
                    if crate::directive::from_comment(c).is_none() {
                        out.push(c.clone());
                    }
                    i -= 1;
                }
                _ => break,
            }
        }
        out.reverse();
        out
    }
}

pub fn parse(content: &str) -> EnvFile {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let src: Vec<&str> = content.lines().collect();
    let mut lines = Vec::new();
    let mut i = 0;

    while i < src.len() {
        let raw = src[i];
        let trimmed = raw.trim();

        if trimmed.is_empty() {
            lines.push(Line::Blank);
            i += 1;
            continue;
        }
        if trimmed.starts_with('#') {
            lines.push(Line::Comment(raw.trim_end().to_string()));
            i += 1;
            continue;
        }

        match parse_entry(&src, i) {
            Ok((entry, consumed)) => {
                lines.push(Line::Entry(entry));
                i += consumed;
            }
            Err(reason) => {
                lines.push(Line::Invalid {
                    line: i + 1,
                    raw: raw.to_string(),
                    reason,
                });
                i += 1;
            }
        }
    }
    EnvFile { lines }
}

fn parse_entry(src: &[&str], i: usize) -> Result<(Entry, usize), String> {
    let trimmed = src[i].trim();

    let (export, rest) = match trimmed.strip_prefix("export") {
        Some(r) if r.starts_with(char::is_whitespace) => (true, r.trim_start()),
        _ => (false, trimmed),
    };

    let eq = rest
        .find('=')
        .ok_or_else(|| "missing '=' sign".to_string())?;
    let key = rest[..eq].trim();
    if key.is_empty() {
        return Err("empty key name".into());
    }
    if key.chars().any(|c| c.is_whitespace()) {
        return Err(format!("key '{key}' contains whitespace"));
    }

    let after = rest[eq + 1..].trim_start();
    let first = after.chars().next();

    if let Some(q @ ('"' | '\'')) = first {
        let mut text = after[1..].to_string();
        let mut consumed = 1;
        loop {
            if let Some(idx) = find_closing(&text, q) {
                let value_raw = &text[..idx];
                let remainder = text[idx + 1..].trim();
                let comment = if remainder.is_empty() {
                    None
                } else if let Some(c) = remainder.strip_prefix('#') {
                    Some(c.trim().to_string())
                } else {
                    return Err("unexpected characters after the closing quote".into());
                };
                let value = if q == '"' {
                    unescape(value_raw)
                } else {
                    value_raw.to_string()
                };
                let raw = src[i..i + consumed].join("\n");
                return Ok((
                    Entry {
                        key: key.to_string(),
                        value,
                        quote: if q == '"' {
                            Quote::Double
                        } else {
                            Quote::Single
                        },
                        export,
                        comment,
                        line: i + 1,
                        raw,
                    },
                    consumed,
                ));
            }
            if i + consumed < src.len() {
                text.push('\n');
                text.push_str(src[i + consumed]);
                consumed += 1;
            } else {
                return Err("unterminated quote".into());
            }
        }
    }

    let (value_part, comment) = split_inline_comment(after);
    Ok((
        Entry {
            key: key.to_string(),
            value: value_part.trim().to_string(),
            quote: Quote::None,
            export,
            comment,
            line: i + 1,
            raw: src[i].to_string(),
        },
        1,
    ))
}

fn find_closing(text: &str, q: char) -> Option<usize> {
    let mut escaped = false;
    for (idx, c) in text.char_indices() {
        if q == '"' && escaped {
            escaped = false;
            continue;
        }
        if q == '"' && c == '\\' {
            escaped = true;
            continue;
        }
        if c == q {
            return Some(idx);
        }
    }
    None
}

fn split_inline_comment(s: &str) -> (&str, Option<String>) {
    if let Some(c) = s.strip_prefix('#') {
        return ("", Some(c.trim().to_string()));
    }
    let bytes = s.as_bytes();
    for (idx, &b) in bytes.iter().enumerate() {
        if b == b'#' && idx > 0 && (bytes[idx - 1] == b' ' || bytes[idx - 1] == b'\t') {
            return (&s[..idx], Some(s[idx + 1..].trim().to_string()));
        }
    }
    (s, None)
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub fn quote_if_needed(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    let needs = value
        .chars()
        .any(|c| c.is_whitespace() || matches!(c, '#' | '"' | '\'' | '\\' | '$' | '`'));
    if !needs {
        return value.to_string();
    }
    let mut out = String::from('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first_entry(src: &str) -> Entry {
        parse(src).entries().next().expect("wpis").clone()
    }

    #[test]
    fn value_with_equals_sign_is_kept() {
        let e = first_entry("TOKEN=abc==\n");
        assert_eq!(e.value, "abc==");
    }

    #[test]
    fn export_and_inline_comment() {
        let e = first_entry("export PORT=3000 # server port\n");
        assert!(e.export);
        assert_eq!(e.value, "3000");
        assert_eq!(e.comment.as_deref(), Some("server port"));
    }

    #[test]
    fn hash_without_space_is_part_of_value() {
        assert_eq!(first_entry("PASS=ab#cd\n").value, "ab#cd");
    }

    #[test]
    fn quoted_multiline() {
        let f = parse("KEY=\"a\nb\"\nNEXT=1\n");
        assert_eq!(f.get("KEY").unwrap().value, "a\nb");
        assert_eq!(f.get("NEXT").unwrap().line, 3);
    }

    #[test]
    fn invalid_lines_are_reported() {
        let f = parse("just text\nA B=1\nQ=\"open\n");
        assert_eq!(f.invalid().count(), 3);
    }

    #[test]
    fn quote_roundtrip() {
        let q = quote_if_needed("a b\"c");
        let e = first_entry(&format!("K={q}\n"));
        assert_eq!(e.value, "a b\"c");
    }
}
