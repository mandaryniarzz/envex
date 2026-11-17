use crate::cli::{GenerateArgs, HookAction, SyncArgs, WatchArgs};
use crate::generate::{self, GenOptions};
use crate::parser;
use crate::ui;
use crate::util::{self, Ctx};
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as Proc;
use std::time::Duration;

pub fn generate(ctx: &Ctx, args: &GenerateArgs) -> Result<i32> {
    let opts = GenOptions::from_config(&ctx.cfg, args.mode, args.no_comments);
    for (env_path, example_path) in util::pairs(ctx, args.all)? {
        if !env_path.exists() {
            bail!(
                "file {} not found – nothing to generate from",
                env_path.display()
            );
        }
        let env = util::read_env(&env_path)?;
        let g = generate::generate(&env, &opts, &ctx.cfg);
        for w in &g.warnings {
            ui::warn(format!("{}: {w}", env_path.display()));
        }
        if args.stdout {
            print!("{}", g.text);
            continue;
        }
        let existing = util::read_text_or_empty(&example_path)?;
        if args.dry_run {
            ui::say(ui::bold(&format!(
                "{} → {}",
                env_path.display(),
                example_path.display()
            )));
            util::print_diff(&existing, &g.text);
            continue;
        }
        if example_path.exists() && !args.force {
            bail!(
                "{} already exists. Use `envex sync` (keeps comments and custom values) or `--force` to overwrite",
                example_path.display()
            );
        }
        util::write_text(&example_path, &g.text)?;
        let count = parser::parse(&g.text).entries().count();
        ui::success(format!("wrote {} ({} keys)", example_path.display(), count));
    }
    Ok(0)
}

fn sync_one(
    ctx: &Ctx,
    env_path: &Path,
    example_path: &Path,
    opts: &GenOptions,
    prune: bool,
    check: bool,
    dry_run: bool,
) -> Result<bool> {
    let env = util::read_env(env_path)?;
    let existing = util::read_text_or_empty(example_path)?;
    let example = parser::parse(&existing);
    let report = generate::sync(&env, &example, opts, &ctx.cfg, prune);

    for w in &report.warnings {
        ui::warn(format!("{}: {w}", env_path.display()));
    }
    let changed = !util::same_text(&existing, &report.text);

    let label = format!("{} → {}", env_path.display(), example_path.display());
    if !changed {
        ui::success(format!("{label}: up to date"));
    } else if check {
        ui::error(format!("{label}: out of date"));
    } else {
        ui::say(ui::bold(&label));
    }
    for k in &report.added {
        ui::say(format!("  {} {k}", ui::green("+")));
    }
    for k in &report.removed {
        ui::say(format!("  {} {k}", ui::red("-")));
    }
    if !report.stale.is_empty() {
        ui::say(ui::dim(&format!(
            "  {} keys in {} no longer exist in {} (use --prune): {}",
            report.stale.len(),
            example_path.display(),
            env_path.display(),
            report.stale.join(", ")
        )));
    }

    if changed && !check {
        if dry_run {
            util::print_diff(&existing, &report.text);
        } else {
            util::write_text(example_path, &report.text)?;
            ui::success(format!("wrote {}", example_path.display()));
        }
    }
    Ok(changed)
}

pub fn sync(ctx: &Ctx, args: &SyncArgs, quiet_missing_env: bool) -> Result<i32> {
    let opts = GenOptions::from_config(&ctx.cfg, args.mode, args.no_comments);
    let mut drift = false;
    let mut written: Vec<PathBuf> = Vec::new();
    for (env_path, example_path) in util::pairs(ctx, args.all)? {
        if !env_path.exists() {
            if quiet_missing_env {
                continue;
            }
            bail!("file {} not found", env_path.display());
        }
        let changed = sync_one(
            ctx,
            &env_path,
            &example_path,
            &opts,
            args.prune,
            args.check,
            args.dry_run,
        )?;
        drift |= changed;
        if changed && !args.check && !args.dry_run {
            written.push(example_path);
        }
    }
    if args.stage && !written.is_empty() {
        let status = Proc::new("git")
            .arg("add")
            .arg("--")
            .args(&written)
            .status()
            .context("failed to run git")?;
        if !status.success() {
            bail!("git add failed");
        }
    }
    Ok(if args.check && drift { 1 } else { 0 })
}

fn mtime(path: &Path) -> Option<std::time::SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

pub fn watch(ctx: &Ctx, args: &WatchArgs, all: bool) -> Result<i32> {
    let opts = GenOptions::from_config(&ctx.cfg, args.mode, false);
    let pairs = util::pairs(ctx, all)?;
    ui::say(format!(
        "{} watching {} (Ctrl+C to stop)",
        ui::cyan("👀"),
        pairs
            .iter()
            .map(|(e, _)| e.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ));
    let mut last: Vec<Option<std::time::SystemTime>> = vec![None; pairs.len()];
    loop {
        for (i, (env_path, example_path)) in pairs.iter().enumerate() {
            let current = mtime(env_path);
            if current != last[i] {
                last[i] = current;
                if current.is_some()
                    && let Err(e) =
                        sync_one(ctx, env_path, example_path, &opts, args.prune, false, false)
                {
                    ui::error(format!("{e:#}"));
                }
            }
        }
        std::thread::sleep(Duration::from_millis(args.interval));
    }
}

const HOOK_MARKER: &str = "# envex-managed-hook";

fn hook_script() -> String {
    format!(
        "#!/bin/sh\n{HOOK_MARKER}\n# Automatically syncs .env.example files before each commit.\nif command -v envex >/dev/null 2>&1; then\n  envex sync --all --quiet --stage || exit 1\nfi\n"
    )
}

fn find_git_dir() -> Result<PathBuf> {
    let mut dir = std::env::current_dir()?;
    loop {
        let candidate = dir.join(".git");
        if candidate.is_dir() {
            return Ok(candidate);
        }
        if !dir.pop() {
            bail!("git repository not found (no .git directory)");
        }
    }
}

pub fn hook(action: &HookAction) -> Result<i32> {
    let git_dir = find_git_dir()?;
    let hooks_dir = git_dir.join("hooks");
    fs::create_dir_all(&hooks_dir)?;
    let path = hooks_dir.join("pre-commit");
    let existing = fs::read_to_string(&path).ok();
    let ours = existing.as_deref().is_some_and(|c| c.contains(HOOK_MARKER));

    match action {
        HookAction::Install { force } => {
            if existing.is_some() && !ours && !force {
                bail!(
                    "another pre-commit hook already exists ({}). Use --force to overwrite it",
                    path.display()
                );
            }
            fs::write(&path, hook_script())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
            }
            ui::success(format!("installed hook: {}", path.display()));
        }
        HookAction::Uninstall => {
            if ours {
                fs::remove_file(&path)?;
                ui::success("removed the pre-commit hook");
            } else if existing.is_some() {
                bail!("the pre-commit hook was not installed by envex – leaving it alone");
            } else {
                ui::say("no hook installed");
            }
        }
        HookAction::Status => {
            let state = match (existing.is_some(), ours) {
                (true, true) => ui::green("installed (envex)"),
                (true, false) => ui::yellow("a pre-commit hook not managed by envex exists"),
                _ => ui::dim("not installed"),
            };
            println!("{state}");
        }
    }
    Ok(0)
}
