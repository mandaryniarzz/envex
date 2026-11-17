use crate::cli::{Cli, ConfigAction, ExportArgs, ExportFormat, FillArgs, RunArgs};
use crate::config;
use crate::generate;
use crate::parser::{self, Line, quote_if_needed};
use crate::secrets;
use crate::ui;
use crate::util::{self, Ctx};
use anyhow::{Context, Result, bail};
use clap::CommandFactory;
use clap_complete::Shell;
use std::collections::HashSet;
use std::io::{BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::process::Command as Proc;

fn prompt(key: &str, default: &str) -> Result<String> {
    let mut stdout = std::io::stdout();
    if default.is_empty() {
        write!(stdout, "{key}: ")?;
    } else {
        write!(stdout, "{key} [{default}]: ")?;
    }
    stdout.flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    let answer = line.trim_end_matches(['\n', '\r']).to_string();
    Ok(if answer.is_empty() {
        default.to_string()
    } else {
        answer
    })
}

pub fn fill(ctx: &Ctx, args: &FillArgs) -> Result<i32> {
    if !ctx.example.exists() {
        bail!("file {} not found", ctx.example.display());
    }
    let out_path = args.output.clone().unwrap_or_else(|| ctx.env.clone());
    let example = util::read_env(&ctx.example)?;
    let existing = if out_path.exists() && !args.force {
        Some(util::read_env(&out_path)?)
    } else {
        None
    };
    let existing_keys: HashSet<String> = existing.as_ref().map(|e| e.key_set()).unwrap_or_default();
    let interactive = !args.yes && std::io::stdin().is_terminal();

    let ask = |entry: &parser::Entry| -> Result<String> {
        let mut default = entry.value.clone();
        if default.is_empty()
            && args.random_secrets
            && secrets::key_looks_secret(&entry.key, &ctx.cfg.secret_patterns)
        {
            default = secrets::random_secret(32)?;
            if interactive {
                return prompt(&entry.key, &format!("{}…", &default[..8])).map(|a| {
                    if a.ends_with('…') {
                        default.clone()
                    } else {
                        a
                    }
                });
            }
            return Ok(default);
        }
        if interactive {
            prompt(&entry.key, &default)
        } else {
            Ok(default)
        }
    };

    let mut added = 0;
    let text = match existing {
        None => {
            let mut lines: Vec<String> = Vec::new();
            for l in &example.lines {
                match l {
                    Line::Blank => lines.push(String::new()),
                    Line::Comment(c) => {
                        if crate::directive::from_comment(c).is_none() {
                            lines.push(c.clone())
                        }
                    }
                    Line::Entry(e) => {
                        let v = ask(e)?;
                        added += 1;
                        lines.push(format!(
                            "{}{}={}",
                            if e.export { "export " } else { "" },
                            e.key,
                            quote_if_needed(&v)
                        ));
                    }
                    Line::Invalid { raw, .. } => lines.push(raw.clone()),
                }
            }
            let mut t = lines.join("\n");
            t.push('\n');
            t
        }
        Some(_) => {
            let mut t = util::read_text_or_empty(&out_path)?;
            let mut block = String::new();
            for (idx, l) in example.lines.iter().enumerate() {
                if let Line::Entry(e) = l {
                    if existing_keys.contains(&e.key) {
                        continue;
                    }
                    for c in example.attached_comments(idx) {
                        block.push_str(&c);
                        block.push('\n');
                    }
                    let v = ask(e)?;
                    block.push_str(&format!("{}={}\n", e.key, quote_if_needed(&v)));
                    added += 1;
                }
            }
            if added > 0 {
                if !t.ends_with('\n') && !t.is_empty() {
                    t.push('\n');
                }
                t.push_str("\n# Added by `envex fill`\n");
                t.push_str(&block);
            }
            t
        }
    };

    if added == 0 {
        ui::success(format!("{} already has all keys", out_path.display()));
        return Ok(0);
    }
    util::write_text(&out_path, &text)?;
    ui::success(format!("wrote {} ({added} keys)", out_path.display()));
    Ok(0)
}

fn shell_quote(v: &str) -> String {
    format!("'{}'", v.replace('\'', r"'\''"))
}

pub fn export(ctx: &Ctx, args: &ExportArgs) -> Result<i32> {
    let env = util::read_env(&ctx.env)?;
    let mut seen = HashSet::new();
    let items: Vec<(&str, &str)> = env
        .entries()
        .filter(|e| seen.insert(e.key.as_str()))
        .map(|e| (e.key.as_str(), e.value.as_str()))
        .collect();
    match args.format {
        ExportFormat::Json => {
            let map: serde_json::Map<String, serde_json::Value> = items
                .iter()
                .map(|(k, v)| (k.to_string(), serde_json::Value::String(v.to_string())))
                .collect();
            println!("{}", serde_json::to_string_pretty(&map)?);
        }
        ExportFormat::Shell => {
            for (k, v) in items {
                println!("export {k}={}", shell_quote(v));
            }
        }
        ExportFormat::Yaml => {
            for (k, v) in items {
                println!("{k}: {}", serde_json::to_string(v)?);
            }
        }
        ExportFormat::Dotenv => {
            for (k, v) in items {
                println!("{k}={}", quote_if_needed(v));
            }
        }
    }
    Ok(0)
}

pub fn run(ctx: &Ctx, args: &RunArgs) -> Result<i32> {
    let env = util::read_env(&ctx.env)?;
    if args.check {
        if !ctx.example.exists() {
            bail!(
                "file {} required by --check not found",
                ctx.example.display()
            );
        }
        let example = util::read_env(&ctx.example)?;
        let cmp = generate::compare(&env, &example, &ctx.cfg);
        if !cmp.missing.is_empty() || !cmp.empty.is_empty() {
            let mut all = cmp.missing.clone();
            all.extend(cmp.empty.clone());
            bail!("missing required variables: {}", all.join(", "));
        }
    }
    let (program, rest) = args.command.split_first().context("no command to run")?;
    let mut cmd = Proc::new(program);
    cmd.args(rest);
    for e in env.entries() {
        if args.override_existing || std::env::var_os(&e.key).is_none() {
            cmd.env(&e.key, &e.value);
        }
    }
    let status = cmd
        .status()
        .with_context(|| format!("failed to run `{program}`"))?;
    Ok(status.code().unwrap_or(1))
}

pub fn completions(shell: Shell) -> Result<i32> {
    let mut cmd = Cli::command();
    clap_complete::generate(shell, &mut cmd, "envex", &mut std::io::stdout());
    Ok(0)
}

pub fn config(ctx: &Ctx, action: &ConfigAction) -> Result<i32> {
    match action {
        ConfigAction::Init { force } => {
            let path = PathBuf::from(config::CONFIG_FILE);
            if path.exists() && !force {
                bail!("{} already exists (use --force)", path.display());
            }
            util::write_text(&path, config::TEMPLATE)?;
            ui::success(format!("created {}", path.display()));
        }
        ConfigAction::Show => {
            println!("{:#?}", ctx.cfg);
        }
        ConfigAction::Path => match &ctx.config_path {
            Some(p) => println!("{}", p.display()),
            None => ui::say("no config file (using defaults)"),
        },
    }
    Ok(0)
}
