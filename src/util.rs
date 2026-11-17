use crate::config::Config;
use crate::parser::{self, EnvFile};
use crate::ui;
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};

pub struct Ctx {
    pub cfg: Config,
    pub config_path: Option<PathBuf>,
    pub env: PathBuf,
    pub example: PathBuf,
}

pub fn read_env(path: &Path) -> Result<EnvFile> {
    let text =
        fs::read_to_string(path).with_context(|| format!("cannot read file {}", path.display()))?;
    Ok(parser::parse(&text))
}

pub fn read_text_or_empty(path: &Path) -> Result<String> {
    if path.exists() {
        fs::read_to_string(path).with_context(|| format!("cannot read file {}", path.display()))
    } else {
        Ok(String::new())
    }
}

pub fn write_text(path: &Path, text: &str) -> Result<()> {
    fs::write(path, text).with_context(|| format!("cannot write file {}", path.display()))
}

fn is_env_file_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    let excluded = [
        "example", "sample", "template", "dist", "defaults", "bak", "backup", "orig",
    ];
    if excluded.iter().any(|e| lower.contains(e)) {
        return false;
    }
    lower == ".env" || lower.starts_with(".env.") || lower.ends_with(".env")
}

pub fn example_path_for(env: &Path) -> PathBuf {
    let mut s = env.as_os_str().to_os_string();
    s.push(".example");
    PathBuf::from(s)
}

pub fn pairs(ctx: &Ctx, all: bool) -> Result<Vec<(PathBuf, PathBuf)>> {
    if !all {
        return Ok(vec![(ctx.env.clone(), ctx.example.clone())]);
    }
    let mut found: Vec<PathBuf> = Vec::new();
    for entry in fs::read_dir(".")? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if is_env_file_name(&name) {
            found.push(PathBuf::from(name));
        }
    }
    found.sort();
    if found.is_empty() {
        bail!("no .env files found in the current directory");
    }
    Ok(found
        .into_iter()
        .map(|e| {
            let ex = if e == Path::new(".env") {
                ctx.example.clone()
            } else {
                example_path_for(&e)
            };
            (e, ex)
        })
        .collect())
}

pub fn mask(value: &str) -> String {
    if value.is_empty() {
        "(empty)".into()
    } else {
        "***".into()
    }
}

pub fn print_diff(old: &str, new: &str) {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let (n, m) = (a.len(), b.len());
    let mut lcs = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut changed = false;
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            i += 1;
            j += 1;
        } else if j < m && (i == n || lcs[i][j + 1] >= lcs[i + 1][j]) {
            println!("{}", ui::green(&format!("+ {}", b[j])));
            changed = true;
            j += 1;
        } else {
            println!("{}", ui::red(&format!("- {}", a[i])));
            changed = true;
            i += 1;
        }
    }
    if !changed {
        println!("{}", ui::dim("(no changes)"));
    }
}

pub fn same_text(a: &str, b: &str) -> bool {
    a.trim_end() == b.trim_end()
}
