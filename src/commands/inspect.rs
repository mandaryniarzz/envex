use crate::cli::{CheckArgs, DiffArgs, Format, LintArgs, ScanArgs};
use crate::generate;
use crate::parser::{EnvFile, Line, Quote};
use crate::secrets;
use crate::ui;
use crate::util::{self, Ctx};
use anyhow::{Result, bail};
use regex::Regex;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as Proc;

pub fn check(ctx: &Ctx, args: &CheckArgs) -> Result<i32> {
    let mut failed = false;
    let mut json_out = Vec::new();
    for (env_path, example_path) in util::pairs(ctx, args.all)? {
        if !example_path.exists() {
            bail!(
                "file {} not found – create it with `envex generate`",
                example_path.display()
            );
        }
        if !env_path.exists() {
            bail!("file {} not found", env_path.display());
        }
        let env = util::read_env(&env_path)?;
        let example = util::read_env(&example_path)?;
        let cmp = generate::compare(&env, &example, &ctx.cfg);
        let ok = cmp.is_clean(args.strict);
        failed |= !ok;

        if args.format == Format::Json {
            json_out.push(json!({
                "env": env_path.display().to_string(),
                "example": example_path.display().to_string(),
                "ok": ok,
                "missing": cmp.missing,
                "missing_optional": cmp.missing_optional,
                "undocumented": cmp.undocumented,
                "empty": cmp.empty,
            }));
            continue;
        }

        ui::say(ui::bold(&format!(
            "{} vs {}",
            env_path.display(),
            example_path.display()
        )));
        if !cmp.missing.is_empty() {
            ui::say(format!(
                "  {} missing in {} ({}): {}",
                ui::red("✘"),
                env_path.display(),
                cmp.missing.len(),
                cmp.missing.join(", ")
            ));
        }
        if !cmp.undocumented.is_empty() {
            let mark = if args.strict {
                ui::red("✘")
            } else {
                ui::yellow("!")
            };
            ui::say(format!(
                "  {mark} not in {} ({}): {}  {}",
                example_path.display(),
                cmp.undocumented.len(),
                cmp.undocumented.join(", "),
                ui::dim("→ envex sync")
            ));
        }
        if !cmp.empty.is_empty() {
            let mark = if args.strict {
                ui::red("✘")
            } else {
                ui::yellow("!")
            };
            ui::say(format!(
                "  {mark} empty values ({}): {}",
                cmp.empty.len(),
                cmp.empty.join(", ")
            ));
        }
        if !cmp.missing_optional.is_empty() {
            ui::say(ui::dim(&format!(
                "  skipped optional: {}",
                cmp.missing_optional.join(", ")
            )));
        }
        if ok && cmp.undocumented.is_empty() && cmp.empty.is_empty() {
            ui::success("everything matches");
        } else if ok {
            ui::say(format!(
                "  {} no critical problems (use --strict to be stricter)",
                ui::green("✔")
            ));
        }
    }
    if args.format == Format::Json {
        let value = if json_out.len() == 1 {
            json_out.remove(0)
        } else {
            serde_json::Value::Array(json_out)
        };
        println!("{}", serde_json::to_string_pretty(&value)?);
    }
    Ok(if failed { 1 } else { 0 })
}

pub fn diff(ctx: &Ctx, args: &DiffArgs) -> Result<i32> {
    let (a_path, b_path) = match args.files.as_slice() {
        [] => (ctx.env.clone(), ctx.example.clone()),
        [a, b] => (a.clone(), b.clone()),
        _ => bail!("provide exactly two files or none"),
    };
    let a = util::read_env(&a_path)?;
    let b = util::read_env(&b_path)?;
    let show = |v: &str| {
        if args.show_values {
            if v.is_empty() {
                "(empty)".to_string()
            } else {
                v.to_string()
            }
        } else {
            util::mask(v)
        }
    };

    ui::say(ui::bold(&format!(
        "--- {}\n+++ {}",
        a_path.display(),
        b_path.display()
    )));
    let mut diffs = 0;
    let mut keys: Vec<String> = a.keys();
    for k in b.keys() {
        if !keys.contains(&k) {
            keys.push(k);
        }
    }
    for k in keys {
        match (a.get(&k), b.get(&k)) {
            (Some(x), None) => {
                diffs += 1;
                println!("{}", ui::red(&format!("- {k}={}", show(&x.value))));
            }
            (None, Some(y)) => {
                diffs += 1;
                println!("{}", ui::green(&format!("+ {k}={}", show(&y.value))));
            }
            (Some(x), Some(y)) if x.value != y.value => {
                diffs += 1;
                println!(
                    "{}",
                    ui::yellow(&format!("~ {k}: {} → {}", show(&x.value), show(&y.value)))
                );
            }
            _ => {}
        }
    }
    if diffs == 0 {
        ui::success("no differences in keys or values");
    }
    Ok(if args.exit_code && diffs > 0 { 1 } else { 0 })
}

#[derive(Debug, PartialEq)]
enum Level {
    Error,
    Warning,
}

struct Finding {
    line: usize,
    level: Level,
    msg: String,
}

fn is_example_name(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    ["example", "sample", "template", "dist"]
        .iter()
        .any(|s| name.contains(s))
}

fn git_ignored(path: &Path) -> Option<bool> {
    let out = Proc::new("git")
        .args(["check-ignore", "-q"])
        .arg(path)
        .output()
        .ok()?;
    match out.status.code() {
        Some(0) => Some(true),
        Some(1) => Some(false),
        _ => None,
    }
}

fn lint_file(path: &Path, file: &EnvFile, ctx: &Ctx) -> Vec<Finding> {
    let mut out = Vec::new();
    let example_like = is_example_name(path);
    let spacing = Regex::new(r"^\s*(export\s+)?[^=\s#]+\s+=|^[^=]*=\s+\S").expect("regex");
    let key_ok = Regex::new(r"^[A-Z][A-Z0-9_]*$").expect("regex");
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();

    for (line, raw, reason) in file.invalid() {
        out.push(Finding {
            line,
            level: Level::Error,
            msg: format!("invalid line ({reason}): {}", raw.trim()),
        });
    }
    for l in &file.lines {
        let Line::Entry(e) = l else { continue };
        if let Some(first) = seen.get(&e.key) {
            out.push(Finding {
                line: e.line,
                level: Level::Warning,
                msg: format!("duplicate key {} (first seen on line {first})", e.key),
            });
        } else {
            seen.insert(e.key.clone(), e.line);
        }
        if !key_ok.is_match(&e.key) {
            out.push(Finding {
                line: e.line,
                level: Level::Warning,
                msg: format!("key {} is not UPPER_SNAKE_CASE", e.key),
            });
        }
        let first_line = e.raw.lines().next().unwrap_or("");
        if e.quote == Quote::None && spacing.is_match(first_line) {
            out.push(Finding {
                line: e.line,
                level: Level::Warning,
                msg: format!("{}: spaces around '=' (some parsers keep them)", e.key),
            });
        }
        if e.quote == Quote::None && e.value.chars().any(char::is_whitespace) {
            out.push(Finding {
                line: e.line,
                level: Level::Warning,
                msg: format!("{}: a value with spaces should be quoted", e.key),
            });
        }
        if e.quote == Quote::None && e.raw.lines().next().is_some_and(|r| r != r.trim_end()) {
            out.push(Finding {
                line: e.line,
                level: Level::Warning,
                msg: format!("{}: trailing whitespace", e.key),
            });
        }
        if example_like {
            let secret = secrets::is_secret(e, &ctx.cfg);
            if !e.value.is_empty() && !secrets::is_placeholder_like(&e.value) && secret {
                out.push(Finding {
                    line: e.line,
                    level: Level::Error,
                    msg: format!("{}: possible secret leak in an example file", e.key),
                });
            }
        }
    }
    if !example_like && git_ignored(path) == Some(false) {
        out.push(Finding {
            line: 0,
            level: Level::Warning,
            msg: "file is not ignored by git – add it to .gitignore".into(),
        });
    }
    out
}

pub fn lint(ctx: &Ctx, args: &LintArgs) -> Result<i32> {
    let files: Vec<PathBuf> = if args.files.is_empty() {
        [ctx.env.clone(), ctx.example.clone()]
            .into_iter()
            .filter(|p| p.exists())
            .collect()
    } else {
        args.files.clone()
    };
    if files.is_empty() {
        bail!("no files to lint were found");
    }
    let (mut errors, mut warnings) = (0, 0);
    for path in &files {
        let file = util::read_env(path)?;
        let findings = lint_file(path, &file, ctx);
        for f in findings {
            let (tag, count) = match f.level {
                Level::Error => (ui::red("error"), &mut errors),
                Level::Warning => (ui::yellow("warn"), &mut warnings),
            };
            *count += 1;
            let loc = if f.line > 0 {
                format!("{}:{}", path.display(), f.line)
            } else {
                path.display().to_string()
            };
            println!("{loc}: {tag}: {}", f.msg);
        }
    }
    if errors == 0 && warnings == 0 {
        ui::success(format!("checked {} file(s) – no issues", files.len()));
    } else {
        ui::say(format!("{errors} error(s), {warnings} warning(s)"));
    }
    Ok(if errors > 0 || (args.deny_warnings && warnings > 0) {
        1
    } else {
        0
    })
}

const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    "vendor",
    ".venv",
    "venv",
    "__pycache__",
    ".next",
    ".nuxt",
    ".idea",
    ".vscode",
    "coverage",
    ".cache",
    ".turbo",
    ".svelte-kit",
];
const CODE_EXT: &[&str] = &[
    "rs", "js", "jsx", "ts", "tsx", "mjs", "cjs", "vue", "svelte", "py", "rb", "go", "php", "java",
    "kt", "cs", "swift", "sh", "bash", "zsh", "yml", "yaml", "toml", "tf", "ex", "exs",
];
const SYSTEM_VARS: &[&str] = &[
    "PATH", "HOME", "USER", "PWD", "OLDPWD", "SHELL", "TERM", "LANG", "TMPDIR", "HOSTNAME",
    "LOGNAME", "EDITOR",
];

fn scan_regexes() -> Vec<Regex> {
    [
        r"process\.env\.([A-Za-z_][A-Za-z0-9_]*)",
        r#"process\.env\[\s*['"`]([A-Za-z_][A-Za-z0-9_]*)['"`]\s*\]"#,
        r"import\.meta\.env\.([A-Za-z_][A-Za-z0-9_]*)",
        r#"env::var(?:_os)?\(\s*"([A-Za-z_][A-Za-z0-9_]*)""#,
        r#"\b(?:option_)?env!\(\s*"([A-Za-z_][A-Za-z0-9_]*)""#,
        r#"os\.environ(?:\.get)?\s*[\[(]\s*['"]([A-Za-z_][A-Za-z0-9_]*)['"]"#,
        r#"os\.getenv\(\s*['"]([A-Za-z_][A-Za-z0-9_]*)['"]"#,
        r#"os\.(?:Getenv|LookupEnv)\(\s*"([A-Za-z_][A-Za-z0-9_]*)""#,
        r#"\bENV(?:\.fetch)?\s*[\[(]\s*['"]([A-Za-z_][A-Za-z0-9_]*)['"]"#,
        r#"\$_(?:ENV|SERVER)\[\s*['"]([A-Za-z_][A-Za-z0-9_]*)['"]\s*\]"#,
        r#"\b(?:getenv|env)\(\s*['"]([A-Za-z_][A-Za-z0-9_]*)['"]"#,
        r#"System\.getenv\(\s*"([A-Za-z_][A-Za-z0-9_]*)""#,
        r#"Environment\.GetEnvironmentVariable\(\s*"([A-Za-z_][A-Za-z0-9_]*)""#,
    ]
    .iter()
    .map(|p| Regex::new(p).expect("regex"))
    .collect()
}

fn walk(dir: &Path, excludes: &HashSet<String>, files: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for entry in rd.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            if SKIP_DIRS.contains(&name.as_str()) || excludes.contains(&name) {
                continue;
            }
            walk(&path, excludes, files);
        } else if ft.is_file() && !excludes.contains(&name) {
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let lower = name.to_lowercase();
            if CODE_EXT.contains(&ext.as_str())
                || lower == "dockerfile"
                || lower.starts_with("dockerfile.")
                || lower.starts_with("docker-compose")
            {
                files.push(path);
            }
        }
    }
}

pub fn scan(ctx: &Ctx, args: &ScanArgs) -> Result<i32> {
    let mut excludes: HashSet<String> = ctx.cfg.scan.exclude.iter().cloned().collect();
    excludes.extend(args.exclude.iter().cloned());
    let ignore_vars: HashSet<&str> = SYSTEM_VARS
        .iter()
        .copied()
        .chain(ctx.cfg.scan.ignore_vars.iter().map(|s| s.as_str()))
        .collect();

    let mut files = Vec::new();
    walk(&args.path, &excludes, &mut files);
    files.sort();

    let regexes = scan_regexes();
    let interpolation = Regex::new(r"\$\{([A-Z_][A-Z0-9_]*)(?::?[-?+][^}]*)?\}").expect("regex");
    let mut usage: BTreeMap<String, Vec<(PathBuf, usize)>> = BTreeMap::new();

    for file in &files {
        let Ok(text) = fs::read_to_string(file) else {
            continue;
        };
        if text.len() > 2_000_000 {
            continue;
        }
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let ext = file
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let interp = matches!(ext.as_str(), "yml" | "yaml" | "sh" | "bash" | "zsh")
            || name.starts_with("dockerfile")
            || name.starts_with("docker-compose");
        for (idx, line) in text.lines().enumerate() {
            for re in &regexes {
                for cap in re.captures_iter(line) {
                    usage
                        .entry(cap[1].to_string())
                        .or_default()
                        .push((file.clone(), idx + 1));
                }
            }
            if interp {
                for cap in interpolation.captures_iter(line) {
                    usage
                        .entry(cap[1].to_string())
                        .or_default()
                        .push((file.clone(), idx + 1));
                }
            }
        }
    }
    usage.retain(|k, _| !ignore_vars.contains(k.as_str()));

    let example = if ctx.example.exists() {
        util::read_env(&ctx.example)?
    } else {
        EnvFile::default()
    };
    let documented = example.key_set();
    let used: BTreeSet<&String> = usage.keys().collect();
    let undocumented: Vec<&String> = used
        .iter()
        .copied()
        .filter(|k| !documented.contains(*k))
        .collect();
    let unused: Vec<String> = example
        .keys()
        .into_iter()
        .filter(|k| !usage.contains_key(k))
        .collect();

    if args.format == Format::Json {
        let used_json: serde_json::Map<String, serde_json::Value> = usage
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    json!(
                        v.iter()
                            .map(|(p, l)| format!("{}:{l}", p.display()))
                            .collect::<Vec<_>>()
                    ),
                )
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(
                &json!({ "files_scanned": files.len(), "used": used_json, "undocumented": undocumented, "unused": unused })
            )?
        );
    } else {
        ui::say(format!(
            "Scanned {} files, found {} variables.",
            files.len(),
            usage.len()
        ));
        if undocumented.is_empty() {
            ui::success(format!(
                "all used variables are in {}",
                ctx.example.display()
            ));
        } else {
            ui::say(ui::bold(&format!(
                "Used in code but missing from {}:",
                ctx.example.display()
            )));
            for k in &undocumented {
                let places = &usage[*k];
                let (p, l) = &places[0];
                let more = if places.len() > 1 {
                    format!(" (+{})", places.len() - 1)
                } else {
                    String::new()
                };
                ui::say(format!(
                    "  {} {k}  {}",
                    ui::red("✘"),
                    ui::dim(&format!("{}:{l}{more}", p.display()))
                ));
            }
        }
        if !unused.is_empty() {
            ui::say(ui::bold(&format!(
                "In {} but not used in code:",
                ctx.example.display()
            )));
            for k in &unused {
                ui::say(format!("  {} {k}", ui::yellow("?")));
            }
        }
    }

    if args.fix && !undocumented.is_empty() {
        let mut text = util::read_text_or_empty(&ctx.example)?;
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str("# Detected by `envex scan`\n");
        for k in &undocumented {
            text.push_str(&format!("{k}=\n"));
        }
        util::write_text(&ctx.example, &text)?;
        ui::success(format!(
            "appended {} keys to {}",
            undocumented.len(),
            ctx.example.display()
        ));
        return Ok(0);
    }
    Ok(if args.check && !undocumented.is_empty() {
        1
    } else {
        0
    })
}
