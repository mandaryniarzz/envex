use crate::parser::Entry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Directive {
    Ignore,

    Keep,

    Optional,
}

fn parse_word(s: &str) -> Option<Directive> {
    match s.trim() {
        "envex:ignore" => Some(Directive::Ignore),
        "envex:keep" => Some(Directive::Keep),
        "envex:optional" => Some(Directive::Optional),
        _ => None,
    }
}

pub fn from_comment(line: &str) -> Option<Directive> {
    parse_word(line.trim().trim_start_matches('#'))
}

pub fn from_inline(entry: &Entry) -> Option<Directive> {
    entry.comment.as_deref().and_then(parse_word)
}
