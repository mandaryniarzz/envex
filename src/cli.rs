use crate::secrets::Mode;
use clap::{Args, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "envex",
    version,
    about = "Generate, sync and validate .env.example files",
    long_about = "envex creates .env.example from .env, keeps both files in sync (preserving \
                  comments and ordering), detects leaked secrets and missing variables, \
                  and scans source code for the environment variables it uses.",
    propagate_version = true,
    arg_required_else_help = true
)]
pub struct Cli {
    /// Path to the .env file (default: from config or `.env`)
    #[arg(short, long, global = true, value_name = "FILE")]
    pub env: Option<PathBuf>,

    /// Path to the .env.example file (default: from config or `.env.example`)
    #[arg(short = 'x', long, global = true, value_name = "FILE")]
    pub example: Option<PathBuf>,

    /// Custom config file (default: `.envex.toml` is searched for)
    #[arg(long, global = true, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Disable colored output
    #[arg(long, global = true)]
    pub no_color: bool,

    /// Print only what is necessary
    #[arg(short, long, global = true)]
    pub quiet: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Generate .env.example from .env
    #[command(visible_aliases = ["gen", "init"])]
    Generate(GenerateArgs),
    /// Sync an existing .env.example with .env (keeps comments and custom values)
    #[command(visible_alias = "s")]
    Sync(SyncArgs),
    /// Check that .env contains every key from .env.example (useful in CI)
    Check(CheckArgs),
    /// Compare two env files (default: .env and .env.example)
    Diff(DiffArgs),
    /// Detect syntax errors, duplicates and leaked secrets
    Lint(LintArgs),
    /// Scan source code for the environment variables it uses
    Scan(ScanArgs),
    /// Create/complete .env from .env.example (interactive)
    Fill(FillArgs),
    /// Watch .env and update .env.example automatically
    Watch(WatchArgs),
    /// Manage the git pre-commit hook
    Hook {
        #[command(subcommand)]
        action: HookAction,
    },
    /// Export variables as JSON / shell / YAML
    Export(ExportArgs),
    /// Run a command with variables loaded from .env
    Run(RunArgs),
    /// Generate a shell completion script
    Completions {
        #[arg(value_enum)]
        shell: Shell,
    },
    /// Manage the .envex.toml config file
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Args, Debug)]
pub struct GenerateArgs {
    /// Value mode for the generated file
    #[arg(short, long, value_enum)]
    pub mode: Option<Mode>,
    /// Overwrite an existing file
    #[arg(short, long)]
    pub force: bool,
    /// Print the result to stdout instead of writing it
    #[arg(long)]
    pub stdout: bool,
    /// Show what would change without writing
    #[arg(short = 'n', long)]
    pub dry_run: bool,
    /// Skip comments
    #[arg(long)]
    pub no_comments: bool,
    /// Process every .env* file in the current directory (.env.local → .env.local.example…)
    #[arg(short, long)]
    pub all: bool,
}

#[derive(Args, Debug)]
pub struct SyncArgs {
    /// Remove keys from .env.example that no longer exist in .env
    #[arg(short, long)]
    pub prune: bool,
    /// Write nothing; exit with code 1 if .env.example is out of date
    #[arg(short, long)]
    pub check: bool,
    /// Show differences without writing
    #[arg(short = 'n', long)]
    pub dry_run: bool,
    /// Value mode for newly added keys
    #[arg(short, long, value_enum)]
    pub mode: Option<Mode>,
    /// Skip comments for newly added keys
    #[arg(long)]
    pub no_comments: bool,
    /// Process every .env* file
    #[arg(short, long)]
    pub all: bool,
    /// Run `git add` on changed files after writing
    #[arg(long)]
    pub stage: bool,
}

#[derive(Copy, Clone, Debug, ValueEnum, PartialEq, Eq)]
pub enum Format {
    Text,
    Json,
}

#[derive(Args, Debug)]
pub struct CheckArgs {
    /// Also treat extra and empty keys as errors
    #[arg(short, long)]
    pub strict: bool,
    /// Output format
    #[arg(short, long, value_enum, default_value = "text")]
    pub format: Format,
    /// Check every .env* file
    #[arg(short, long)]
    pub all: bool,
}

#[derive(Args, Debug)]
pub struct DiffArgs {
    /// Two files to compare (default: .env and .env.example)
    #[arg(num_args = 0..=2, value_name = "FILE")]
    pub files: Vec<PathBuf>,
    /// Show values (masked by default)
    #[arg(long)]
    pub show_values: bool,
    /// Exit with code 1 when the files differ
    #[arg(long)]
    pub exit_code: bool,
}

#[derive(Args, Debug)]
pub struct LintArgs {
    /// Files to lint (default: .env and .env.example)
    #[arg(value_name = "FILE")]
    pub files: Vec<PathBuf>,
    /// Warnings also cause exit code 1
    #[arg(long)]
    pub deny_warnings: bool,
}

#[derive(Args, Debug)]
pub struct ScanArgs {
    /// Directory to scan
    #[arg(default_value = ".")]
    pub path: PathBuf,
    /// Append missing variables to .env.example
    #[arg(long)]
    pub fix: bool,
    /// Exit with code 1 if the code uses variables missing from .env.example
    #[arg(short, long)]
    pub check: bool,
    /// Additional directories/files to skip
    #[arg(long, value_name = "NAME")]
    pub exclude: Vec<String>,
    /// Output format
    #[arg(short, long, value_enum, default_value = "text")]
    pub format: Format,
}

#[derive(Args, Debug)]
pub struct FillArgs {
    /// Output file (default: the .env path)
    #[arg(short, long, value_name = "FILE")]
    pub output: Option<PathBuf>,
    /// Do not prompt – use the defaults from .env.example
    #[arg(short, long)]
    pub yes: bool,
    /// Rebuild the file instead of only appending missing keys
    #[arg(short, long)]
    pub force: bool,
    /// Generate random values for empty keys that look like secrets
    #[arg(short, long)]
    pub random_secrets: bool,
}

#[derive(Args, Debug)]
pub struct WatchArgs {
    /// Remove keys that no longer exist in .env
    #[arg(short, long)]
    pub prune: bool,
    /// Polling interval in milliseconds
    #[arg(short, long, default_value_t = 500)]
    pub interval: u64,
    /// Value mode for new keys
    #[arg(short, long, value_enum)]
    pub mode: Option<Mode>,
    /// Watch every .env* file
    #[arg(short, long)]
    pub all: bool,
}

#[derive(Subcommand, Debug)]
pub enum HookAction {
    /// Install the pre-commit hook (automatic sync + git add)
    Install {
        /// Overwrite an existing hook not created by envex
        #[arg(short, long)]
        force: bool,
    },
    /// Remove the hook installed by envex
    Uninstall,
    /// Show the hook status
    Status,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum ExportFormat {
    Json,
    Shell,
    Yaml,
    Dotenv,
}

#[derive(Args, Debug)]
pub struct ExportArgs {
    /// Export format
    #[arg(short, long, value_enum, default_value = "json")]
    pub format: ExportFormat,
}

#[derive(Args, Debug)]
pub struct RunArgs {
    /// First check that .env contains all required keys from .env.example
    #[arg(short, long)]
    pub check: bool,
    /// Values from the file override already-set environment variables
    #[arg(short, long)]
    pub override_existing: bool,
    /// Command to run (after `--`)
    #[arg(
        required = true,
        trailing_var_arg = true,
        allow_hyphen_values = true,
        value_name = "COMMAND"
    )]
    pub command: Vec<String>,
}

#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Create a sample .envex.toml
    Init {
        #[arg(short, long)]
        force: bool,
    },
    /// Show the effective configuration
    Show,
    /// Show the path of the config file in use
    Path,
}
