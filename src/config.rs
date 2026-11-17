use crate::secrets::Mode;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const CONFIG_FILE: &str = ".envex.toml";

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub env: String,
    pub example: String,
    pub mode: Mode,
    pub keep_comments: bool,
    pub keep_inline_comments: bool,

    pub secret_patterns: Vec<String>,

    pub ignore_keys: Vec<String>,

    pub placeholders: BTreeMap<String, String>,
    pub scan: ScanConfig,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct ScanConfig {
    pub exclude: Vec<String>,

    pub ignore_vars: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            env: ".env".into(),
            example: ".env.example".into(),
            mode: Mode::Empty,
            keep_comments: true,
            keep_inline_comments: true,
            secret_patterns: vec![],
            ignore_keys: vec![],
            placeholders: BTreeMap::new(),
            scan: ScanConfig::default(),
        }
    }
}

pub fn find_config(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start);
    while let Some(d) = dir {
        let candidate = d.join(CONFIG_FILE);
        if candidate.is_file() {
            return Some(candidate);
        }
        dir = d.parent();
    }
    None
}

pub fn load(explicit: Option<&Path>) -> Result<(Config, Option<PathBuf>)> {
    let path = match explicit {
        Some(p) => Some(p.to_path_buf()),
        None => find_config(&std::env::current_dir()?),
    };
    match path {
        Some(p) => {
            let text = std::fs::read_to_string(&p)
                .with_context(|| format!("cannot read {}", p.display()))?;
            let cfg: Config = toml::from_str(&text)
                .with_context(|| format!("invalid configuration in {}", p.display()))?;
            Ok((cfg, Some(p)))
        }
        None => Ok((Config::default(), None)),
    }
}

pub const TEMPLATE: &str = r#"# envex configuration

# Default paths
env = ".env"
example = ".env.example"

# Value mode w .env.example:
#   empty       - empty values (KEY=)
#   safe        - keeps non-sensitive values (numbers, true/false, localhost...)
#   placeholder - your_key_name / change_me
#   typed       - detects the type (URL, e-mail...) and inserts an example
mode = "empty"

keep_comments = true
keep_inline_comments = true

# Extra key-name fragments treated as secrets
secret_patterns = []

# Keys skipped in .env.example
ignore_keys = []

# Fixed values in .env.example for selected keys
[placeholders]
# DATABASE_URL = "postgres://user:password@localhost:5432/app"

[scan]
# exclude = ["generated", "legacy"]
# ignore_vars = ["CI", "GITHUB_ACTIONS"]
"#;
