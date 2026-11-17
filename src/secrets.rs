use crate::config::Config;
use crate::parser::{Entry, quote_if_needed};
use anyhow::{Result, anyhow};
use clap::ValueEnum;
use regex::Regex;
use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Empty,

    Safe,

    Placeholder,

    Typed,
}

const SECRET_TOKENS: &[&str] = &[
    "SECRET",
    "TOKEN",
    "PASSWORD",
    "PASSWD",
    "PASS",
    "PWD",
    "PRIVATE",
    "CREDENTIAL",
    "CREDENTIALS",
    "KEY",
    "APIKEY",
    "AUTH",
    "SALT",
    "JWT",
    "DSN",
    "SIGNATURE",
    "COOKIE",
    "CERT",
    "BEARER",
];

pub fn key_looks_secret(key: &str, extra: &[String]) -> bool {
    let upper = key.to_uppercase();
    if extra.iter().any(|p| upper.contains(&p.to_uppercase())) {
        return true;
    }
    let tokens: Vec<&str> = upper.split(|c: char| !c.is_ascii_alphanumeric()).collect();
    if tokens.contains(&"PUBLIC") && !tokens.contains(&"PRIVATE") && !tokens.contains(&"SECRET") {
        return false;
    }
    tokens.iter().any(|t| SECRET_TOKENS.contains(t))
        || ["SECRET", "PASSWORD", "TOKEN", "APIKEY", "PRIVATEKEY"]
            .iter()
            .any(|s| upper.contains(s))
}

fn token_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^(sk_(live|test)_|pk_live_|rk_live_|ghp_|gho_|ghs_|github_pat_|glpat-|xox[abprs]-|AKIA[0-9A-Z]{16}|AIza[0-9A-Za-z_\-]{20,}|eyJ[A-Za-z0-9_\-]+\.eyJ|-----BEGIN [A-Z ]*PRIVATE KEY)",
        )
        .expect("regex")
    })
}

pub fn value_looks_secret(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    if token_regex().is_match(value) {
        return true;
    }
    if url_has_password(value) {
        return true;
    }

    value.len() >= 32
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | '_' | '-'))
        && value.chars().any(|c| c.is_ascii_digit())
        && value.chars().any(|c| c.is_ascii_alphabetic())
}

pub fn is_secret(entry: &Entry, cfg: &Config) -> bool {
    key_looks_secret(&entry.key, &cfg.secret_patterns) || value_looks_secret(&entry.value)
}

pub fn is_placeholder_like(value: &str) -> bool {
    let v = value.trim().to_lowercase();
    v.is_empty()
        || v.starts_with("your")
        || v.starts_with('<')
        || v.starts_with("${")
        || v.contains("change_me")
        || v.contains("changeme")
        || v.contains("example")
        || v.contains("placeholder")
        || v.chars().all(|c| c == 'x' || c == '*')
}

fn url_parts(value: &str) -> Option<(&str, &str)> {
    let (scheme, rest) = value.split_once("://")?;
    if scheme.is_empty()
        || !scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    {
        return None;
    }
    Some((scheme, rest))
}

fn url_has_password(value: &str) -> bool {
    match url_parts(value) {
        Some((_, rest)) => {
            let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
            match authority.rsplit_once('@') {
                Some((userinfo, _)) => {
                    userinfo.contains(':')
                        && !is_placeholder_like(userinfo.split_once(':').unwrap().1)
                }
                None => false,
            }
        }
        None => false,
    }
}

fn is_local_host(host: &str) -> bool {
    let h = host.split(':').next().unwrap_or("");
    matches!(h, "localhost" | "127.0.0.1" | "0.0.0.0" | "::1")
}

fn is_boolish(v: &str) -> bool {
    matches!(
        v.to_lowercase().as_str(),
        "true" | "false" | "yes" | "no" | "on" | "off" | "0" | "1"
    )
}

fn is_number(v: &str) -> bool {
    !v.is_empty() && v.parse::<f64>().is_ok()
}

fn is_env_word(v: &str) -> bool {
    matches!(
        v.to_lowercase().as_str(),
        "development"
            | "dev"
            | "production"
            | "prod"
            | "test"
            | "staging"
            | "debug"
            | "info"
            | "warn"
            | "error"
            | "trace"
    )
}

fn is_email(v: &str) -> bool {
    match v.split_once('@') {
        Some((l, d)) => {
            !l.is_empty()
                && d.contains('.')
                && !v.contains(char::is_whitespace)
                && !v.contains("://")
        }
        None => false,
    }
}

fn is_safe_value(v: &str) -> bool {
    if is_boolish(v) || is_number(v) || is_env_word(v) {
        return true;
    }
    if let Some((_, rest)) = url_parts(v) {
        let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
        return !authority.contains('@') && is_local_host(authority);
    }
    false
}

fn typed_placeholder(entry: &Entry) -> String {
    let v = entry.value.as_str();
    if let Some((scheme, rest)) = url_parts(v) {
        let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
        let host = authority
            .rsplit_once('@')
            .map(|(_, h)| h)
            .unwrap_or(authority);
        let has_creds = authority.contains('@');
        let host = if is_local_host(host) {
            host
        } else {
            "example.com"
        };
        let path = rest[authority.len()..]
            .split(['?', '#'])
            .next()
            .unwrap_or("");
        return if has_creds {
            format!("{scheme}://user:password@{host}{path}")
        } else {
            format!("{scheme}://{host}{path}")
        };
    }
    if is_email(v) {
        return "user@example.com".into();
    }
    if is_boolish(v) || is_number(v) || is_env_word(v) {
        return v.to_string();
    }
    String::new()
}

pub fn placeholder_for(entry: &Entry, mode: Mode, cfg: &Config) -> String {
    if let Some(custom) = cfg.placeholders.get(&entry.key) {
        return quote_if_needed(custom);
    }
    let secret = is_secret(entry, cfg);
    let value = match mode {
        Mode::Empty => String::new(),
        Mode::Safe => {
            if secret || !is_safe_value(&entry.value) {
                String::new()
            } else {
                entry.value.clone()
            }
        }
        Mode::Placeholder => {
            if entry.value.is_empty() {
                String::new()
            } else if secret {
                "change_me".to_string()
            } else {
                format!("your_{}", entry.key.to_lowercase())
            }
        }
        Mode::Typed => {
            if url_parts(&entry.value).is_some() {
                typed_placeholder(entry)
            } else if secret {
                "change_me".to_string()
            } else {
                typed_placeholder(entry)
            }
        }
    };
    quote_if_needed(&value)
}

pub fn random_secret(bytes: usize) -> Result<String> {
    let mut buf = vec![0u8; bytes];
    getrandom::getrandom(&mut buf).map_err(|e| anyhow!("failed to obtain randomness: {e}"))?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    fn entry(line: &str) -> Entry {
        parse(line).entries().next().unwrap().clone()
    }

    #[test]
    fn detects_secret_keys() {
        assert!(key_looks_secret("DB_PASSWORD", &[]));
        assert!(key_looks_secret("STRIPE_API_KEY", &[]));
        assert!(!key_looks_secret("PUBLIC_KEY", &[]));
        assert!(!key_looks_secret("PORT", &[]));
    }

    #[test]
    fn safe_mode_keeps_only_harmless_values() {
        let cfg = Config::default();
        assert_eq!(
            placeholder_for(&entry("PORT=3000"), Mode::Safe, &cfg),
            "3000"
        );
        assert_eq!(
            placeholder_for(&entry("API_TOKEN=abc"), Mode::Safe, &cfg),
            ""
        );
        assert_eq!(
            placeholder_for(&entry("HOST=prod.internal"), Mode::Safe, &cfg),
            ""
        );
        assert_eq!(
            placeholder_for(&entry("DB=postgres://u:p@localhost/db"), Mode::Safe, &cfg),
            ""
        );
    }

    #[test]
    fn typed_mode_rewrites_urls() {
        let cfg = Config::default();
        let out = placeholder_for(
            &entry("DB=postgres://admin:s3cret@db.prod.com:5432/app"),
            Mode::Typed,
            &cfg,
        );
        assert_eq!(out, "postgres://user:password@example.com/app");
    }

    #[test]
    fn detects_token_values() {
        assert!(value_looks_secret("ghp_abcdefghijklmnop"));
        assert!(!value_looks_secret("hello"));
    }
}
