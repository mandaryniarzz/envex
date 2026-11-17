mod cli;
mod commands;
mod config;
mod directive;
mod generate;
mod parser;
mod secrets;
mod ui;
mod util;

use clap::Parser;
use cli::{Cli, Command};
use commands::{inspect, runtime, sync_cmds};
use util::Ctx;

fn run() -> anyhow::Result<i32> {
    let cli = Cli::parse();
    ui::init(cli.no_color, cli.quiet);

    let (cfg, config_path) = config::load(cli.config.as_deref())?;
    let ctx = Ctx {
        env: cli.env.clone().unwrap_or_else(|| cfg.env.clone().into()),
        example: cli
            .example
            .clone()
            .unwrap_or_else(|| cfg.example.clone().into()),
        cfg,
        config_path,
    };

    match &cli.command {
        Command::Generate(a) => sync_cmds::generate(&ctx, a),

        Command::Sync(a) => sync_cmds::sync(&ctx, a, a.stage),
        Command::Check(a) => inspect::check(&ctx, a),
        Command::Diff(a) => inspect::diff(&ctx, a),
        Command::Lint(a) => inspect::lint(&ctx, a),
        Command::Scan(a) => inspect::scan(&ctx, a),
        Command::Fill(a) => runtime::fill(&ctx, a),
        Command::Watch(a) => sync_cmds::watch(&ctx, a, a.all),
        Command::Hook { action } => sync_cmds::hook(action),
        Command::Export(a) => runtime::export(&ctx, a),
        Command::Run(a) => runtime::run(&ctx, a),
        Command::Completions { shell } => runtime::completions(*shell),
        Command::Config { action } => runtime::config(&ctx, action),
    }
}

fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            ui::error(format!("{e:#}"));
            std::process::exit(2);
        }
    }
}
