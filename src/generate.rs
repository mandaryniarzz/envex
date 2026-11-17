use crate::config::Config;
use crate::directive::{self, Directive};
use crate::parser::{Entry, EnvFile, Line};
use crate::secrets::{Mode, placeholder_for};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct GenOptions {
    pub mode: Mode,
    pub keep_comments: bool,
}

impl GenOptions {
    pub fn from_config(cfg: &Config, mode: Option<Mode>, no_comments: bool) -> Self {
        GenOptions {
            mode: mode.unwrap_or(cfg.mode),
            keep_comments: cfg.keep_comments && !no_comments,
        }
    }
}

fn entry_directive(entry: &Entry, pending: Option<Directive>) -> Option<Directive> {
    directive::from_inline(entry).or(pending)
}

fn render_entry(entry: &Entry, value: &str, cfg: &Config) -> String {
    let mut s = String::new();
    if entry.export {
        s.push_str("export ");
    }
    s.push_str(&entry.key);
    s.push('=');
    s.push_str(value);
    if cfg.keep_inline_comments
        && let Some(c) = &entry.comment
        && !c.is_empty()
        && directive::from_inline(entry).is_none()
    {
        s.push_str(" # ");
        s.push_str(c);
    }
    s
}

fn value_for(
    entry: &Entry,
    directive: Option<Directive>,
    opts: &GenOptions,
    cfg: &Config,
) -> String {
    match directive {
        Some(Directive::Keep) => crate::parser::quote_if_needed(&entry.value),
        _ => placeholder_for(entry, opts.mode, cfg),
    }
}

pub struct Generated {
    pub text: String,
    pub warnings: Vec<String>,
}

fn finish(mut lines: Vec<String>) -> String {
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    if lines.is_empty() {
        return String::new();
    }
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

pub fn generate(env: &EnvFile, opts: &GenOptions, cfg: &Config) -> Generated {
    let mut out: Vec<String> = Vec::new();
    let mut warnings = Vec::new();
    let mut seen = HashSet::new();
    let mut pending: Option<Directive> = None;

    for line in &env.lines {
        match line {
            Line::Blank => {
                if out.last().is_some_and(|l| !l.is_empty()) {
                    out.push(String::new());
                }
            }
            Line::Comment(c) => {
                if let Some(d) = directive::from_comment(c) {
                    pending = Some(d);
                } else if opts.keep_comments {
                    out.push(c.clone());
                }
            }
            Line::Entry(e) => {
                let d = entry_directive(e, pending.take());
                if d == Some(Directive::Ignore) || cfg.ignore_keys.contains(&e.key) {
                    continue;
                }
                if !seen.insert(e.key.clone()) {
                    warnings.push(format!(
                        "line {}: duplicate key {} – skipped",
                        e.line, e.key
                    ));
                    continue;
                }
                let value = value_for(e, d, opts, cfg);
                out.push(render_entry(e, &value, cfg));
            }
            Line::Invalid { line, reason, .. } => {
                warnings.push(format!("line {line}: {reason} – skipped"));
            }
        }
    }
    Generated {
        text: finish(out),
        warnings,
    }
}

#[derive(Debug, Default)]
pub struct SyncReport {
    pub text: String,
    pub added: Vec<String>,
    pub removed: Vec<String>,

    pub stale: Vec<String>,
    pub warnings: Vec<String>,
}

struct Item {
    text: String,
    key: Option<String>,
    comment: bool,
}

pub fn sync(
    env: &EnvFile,
    example: &EnvFile,
    opts: &GenOptions,
    cfg: &Config,
    prune: bool,
) -> SyncReport {
    let mut report = SyncReport::default();

    struct Wanted {
        key: String,
        line_text: String,
        comments: Vec<String>,
    }
    let mut wanted: Vec<Wanted> = Vec::new();
    let mut seen = HashSet::new();
    let mut pending: Option<Directive> = None;
    let mut env_keys_all: HashSet<String> = HashSet::new();

    for (idx, line) in env.lines.iter().enumerate() {
        match line {
            Line::Comment(c) => {
                if let Some(d) = directive::from_comment(c) {
                    pending = Some(d);
                }
            }
            Line::Entry(e) => {
                env_keys_all.insert(e.key.clone());
                let d = entry_directive(e, pending.take());
                if d == Some(Directive::Ignore) || cfg.ignore_keys.contains(&e.key) {
                    continue;
                }
                if !seen.insert(e.key.clone()) {
                    report.warnings.push(format!(
                        "line {}: duplicate key {} – skipped",
                        e.line, e.key
                    ));
                    continue;
                }
                let value = value_for(e, d, opts, cfg);
                let comments = if opts.keep_comments {
                    env.attached_comments(idx)
                } else {
                    vec![]
                };
                wanted.push(Wanted {
                    key: e.key.clone(),
                    line_text: render_entry(e, &value, cfg),
                    comments,
                });
            }
            Line::Invalid { line, reason, .. } => {
                report
                    .warnings
                    .push(format!("line {line}: {reason} – skipped"));
            }
            Line::Blank => {}
        }
    }

    let mut items: Vec<Item> = example
        .lines
        .iter()
        .map(|l| match l {
            Line::Blank => Item {
                text: String::new(),
                key: None,
                comment: false,
            },
            Line::Comment(c) => Item {
                text: c.clone(),
                key: None,
                comment: true,
            },
            Line::Entry(e) => Item {
                text: e.raw.clone(),
                key: Some(e.key.clone()),
                comment: false,
            },
            Line::Invalid { raw, .. } => Item {
                text: raw.clone(),
                key: None,
                comment: false,
            },
        })
        .collect();

    let example_keys = example.key_set();

    let stale: Vec<String> = example
        .keys()
        .into_iter()
        .filter(|k| !env_keys_all.contains(k) || cfg.ignore_keys.contains(k))
        .collect();
    if prune {
        let stale_set: HashSet<&str> = stale.iter().map(|s| s.as_str()).collect();
        let mut remove = vec![false; items.len()];
        for (i, it) in items.iter().enumerate() {
            if it.key.as_deref().is_some_and(|k| stale_set.contains(k)) {
                remove[i] = true;
                let mut j = i;
                while j > 0 && items[j - 1].comment {
                    remove[j - 1] = true;
                    j -= 1;
                }
            }
        }
        let mut idx = 0;
        items.retain(|_| {
            let keep = !remove[idx];
            idx += 1;
            keep
        });
        report.removed = stale;
    } else {
        report.stale = stale;
    }

    for (pos, w) in wanted.iter().enumerate() {
        if example_keys.contains(&w.key) {
            continue;
        }
        let mut block: Vec<Item> = w
            .comments
            .iter()
            .map(|c| Item {
                text: c.clone(),
                key: None,
                comment: true,
            })
            .collect();
        block.push(Item {
            text: w.line_text.clone(),
            key: Some(w.key.clone()),
            comment: false,
        });

        let find =
            |key: &str, items: &[Item]| items.iter().position(|it| it.key.as_deref() == Some(key));

        let predecessor = wanted[..pos]
            .iter()
            .rev()
            .find_map(|p| find(&p.key, &items));
        let insert_at = if let Some(i) = predecessor {
            i + 1
        } else if let Some(i) = wanted[pos + 1..].iter().find_map(|n| find(&n.key, &items)) {
            let mut j = i;
            while j > 0 && items[j - 1].comment {
                j -= 1;
            }
            j
        } else {
            if items.last().is_some_and(|it| !it.text.is_empty()) {
                items.push(Item {
                    text: String::new(),
                    key: None,
                    comment: false,
                });
            }
            items.len()
        };
        for (offset, it) in block.into_iter().enumerate() {
            items.insert(insert_at + offset, it);
        }
        report.added.push(w.key.clone());
    }

    report.text = finish(items.into_iter().map(|i| i.text).collect());
    report
}

#[derive(Debug, Default)]
pub struct Comparison {
    pub missing: Vec<String>,

    pub missing_optional: Vec<String>,

    pub undocumented: Vec<String>,

    pub empty: Vec<String>,
}

impl Comparison {
    pub fn is_clean(&self, strict: bool) -> bool {
        self.missing.is_empty()
            && (!strict || (self.undocumented.is_empty() && self.empty.is_empty()))
    }
}

pub fn compare(env: &EnvFile, example: &EnvFile, cfg: &Config) -> Comparison {
    let mut optional: HashSet<String> = HashSet::new();
    let mut pending = None;
    for line in &example.lines {
        match line {
            Line::Comment(c) => {
                if let Some(d) = directive::from_comment(c) {
                    pending = Some(d);
                }
            }
            Line::Entry(e) if entry_directive(e, pending.take()) == Some(Directive::Optional) => {
                optional.insert(e.key.clone());
            }
            _ => {}
        }
    }

    let env_map: HashMap<&str, &Entry> = env.entries().map(|e| (e.key.as_str(), e)).collect();
    let example_keys = example.keys();
    let example_set: HashSet<&str> = example_keys.iter().map(|s| s.as_str()).collect();
    let mut cmp = Comparison::default();

    for key in &example_keys {
        match env_map.get(key.as_str()) {
            None => {
                if optional.contains(key) {
                    cmp.missing_optional.push(key.clone());
                } else {
                    cmp.missing.push(key.clone());
                }
            }
            Some(e) => {
                if e.value.is_empty() && !optional.contains(key) {
                    cmp.empty.push(key.clone());
                }
            }
        }
    }
    for key in env.keys() {
        if !example_set.contains(key.as_str()) && !cfg.ignore_keys.contains(&key) {
            cmp.undocumented.push(key);
        }
    }
    cmp
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    fn opts() -> GenOptions {
        GenOptions {
            mode: Mode::Empty,
            keep_comments: true,
        }
    }

    #[test]
    fn generate_keeps_comments_and_values_with_equals() {
        let env = parse("# DB\nURL=a=b\n\nSECRET=x # note\n");
        let g = generate(&env, &opts(), &Config::default());
        assert_eq!(g.text, "# DB\nURL=\n\nSECRET= # note\n");
    }

    #[test]
    fn ignore_directive_skips_key() {
        let env = parse("# envex:ignore\nLOCAL=1\nA=2\n");
        let g = generate(&env, &opts(), &Config::default());
        assert_eq!(g.text, "A=\n");
    }

    #[test]
    fn keep_directive_copies_value() {
        let env = parse("NODE_ENV=production # envex:keep\n");
        let g = generate(&env, &opts(), &Config::default());
        assert_eq!(g.text, "NODE_ENV=production\n");
    }

    #[test]
    fn sync_inserts_new_key_in_place_and_keeps_custom_values() {
        let env = parse("A=1\n# new\nB=2\nC=3\n");
        let ex = parse("# mine\nA=custom\nC=\n");
        let r = sync(&env, &ex, &opts(), &Config::default(), false);
        assert_eq!(r.text, "# mine\nA=custom\n# new\nB=\nC=\n");
        assert_eq!(r.added, vec!["B"]);
    }

    #[test]
    fn sync_prune_removes_stale_with_comments() {
        let env = parse("A=1\n");
        let ex = parse("A=\n# old\nOLD=\n");
        let kept = sync(&env, &ex, &opts(), &Config::default(), false);
        assert_eq!(kept.stale, vec!["OLD"]);
        let pruned = sync(&env, &ex, &opts(), &Config::default(), true);
        assert_eq!(pruned.text, "A=\n");
        assert_eq!(pruned.removed, vec!["OLD"]);
    }

    #[test]
    fn compare_reports_differences() {
        let env = parse("A=1\nB=\nX=1\n");
        let ex = parse("A=\nB=\nC=\n# envex:optional\nD=\n");
        let c = compare(&env, &ex, &Config::default());
        assert_eq!(c.missing, vec!["C"]);
        assert_eq!(c.missing_optional, vec!["D"]);
        assert_eq!(c.undocumented, vec!["X"]);
        assert_eq!(c.empty, vec!["B"]);
    }
}
