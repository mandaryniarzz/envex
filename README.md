<div align="center">

# 🔐 envex

**Generate, sync and guard your `.env.example` files with a single command.**

[![Rust](https://img.shields.io/badge/Rust-1.85%2B-orange?logo=rust)](https://www.rust-lang.org/)
[![CLI](https://img.shields.io/badge/type-CLI-blue)](#-commands)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](#-license)

</div>

---

`envex` is a fast CLI tool written in Rust. It creates `.env.example` from your `.env`, **keeps both files in sync** without losing your comments or ordering, detects **leaked secrets**, and checks that your code does not use environment variables nobody documented.

## ✨ Features

- 🧬 **Generate** `.env.example` from `.env` in four value modes
- 🔄 **Smart sync** – new keys land in the right place, your manual edits and comments stay
- ✅ **CI-friendly check** – exit code `1` when `.env` lacks keys or `.env.example` is out of date
- 🕵️ **Secret linting** – spots tokens (GitHub, Stripe, AWS, JWT…) and passwords in URLs inside example files
- 🔎 **Code scanner** – JS/TS, Rust, Python, Go, Ruby, PHP, Java, C#, Docker, YAML
- 🪝 **Git pre-commit hook** – `.env.example` updates itself on every commit
- 👀 **Watch mode**, interactive **fill**, **run**, **export**, shell completions
- 🧩 Correct dotenv parser: `export`, quotes, multi-line values, inline comments, values containing `=`

## 📦 Installation

```sh
git clone <repository-url> envex
cd envex
cargo install --path .
```

The `envex` binary is installed to `~/.cargo/bin` – make sure that directory is in your `PATH`:

```sh
export PATH="$HOME/.cargo/bin:$PATH"   # add to ~/.bashrc or ~/.zshrc
envex --version
```

## 🚀 Quick start

```sh
envex generate        # first run: .env  →  .env.example
envex sync            # later: update it without losing comments
envex check           # does .env contain every required key?
envex hook install    # automate it on every commit
```

Given this `.env`:

```env
# Database
DATABASE_URL=postgres://admin:s3cret@db.prod.com:5432/app
PORT=3000
JWT_SECRET=abcd1234abcd1234abcd1234abcd1234
```

`envex generate --mode typed` produces:

```env
# Database
DATABASE_URL=postgres://user:password@example.com/app
PORT=3000
JWT_SECRET=change_me
```

## 🧰 Commands

| Command | What it does |
|---|---|
| `envex generate` (`gen`, `init`) | creates `.env.example` from `.env` · `--mode` `--stdout` `--dry-run` `--force` `--all` |
| `envex sync` (`s`) | updates an existing `.env.example` · `--prune` `--check` `--dry-run` `--stage` |
| `envex check` | compares `.env` with `.env.example` · `--strict` `--format json` |
| `envex diff [A B]` | differences between two env files, values masked · `--show-values` `--exit-code` |
| `envex lint` | syntax, duplicates, naming style, leaked secrets · `--deny-warnings` |
| `envex scan [DIR]` | variables used in code vs `.env.example` · `--fix` `--check` `--exclude` |
| `envex fill` | creates/completes `.env` from `.env.example` · `--yes` `--random-secrets` `--force` |
| `envex watch` | watches `.env` and syncs on every change · `--prune` `--interval` `--all` |
| `envex hook install\|uninstall\|status` | git pre-commit hook |
| `envex run -- <command>` | runs a command with variables from `.env` · `--check` `--override-existing` |
| `envex export` | export as `json`, `shell`, `yaml`, `dotenv` · `-f` |
| `envex completions <shell>` | bash, zsh, fish, powershell, elvish |
| `envex config init\|show\|path` | manage `.envex.toml` |

**Global options:** `-e/--env <FILE>` · `-x/--example <FILE>` · `--config <FILE>` · `-q/--quiet` · `--no-color`

Multiple environments at once: `envex sync --all` handles `.env`, `.env.local`, `.env.production` → `*.example`.

## 🎛️ Value modes

| Mode | Result | Example |
|---|---|---|
| `empty` *(default)* | empty values | `PORT=` |
| `safe` | keeps harmless values (numbers, booleans, `localhost`, `development`…); secrets always empty | `PORT=3000` |
| `placeholder` | `your_key_name`, or `change_me` for secrets | `API_URL=your_api_url` |
| `typed` | detects the type: URLs, e-mails, numbers | `DB=postgres://user:password@example.com/app` |

## 🏷️ Comment directives

```env
# envex:ignore
LOCAL_DEBUG=1                      # will not appear in .env.example

NODE_ENV=production # envex:keep   # the value is copied as is

# in .env.example:
# envex:optional
SENTRY_DSN=                        # a missing or empty value will not fail `check`
```

## ⚙️ Configuration

`envex config init` creates `.envex.toml` (searched for in the current directory and its parents):

```toml
mode = "safe"
ignore_keys = ["LOCAL_ONLY"]
secret_patterns = ["INTERNAL"]

[placeholders]
DATABASE_URL = "postgres://user:password@localhost:5432/app"

[scan]
exclude = ["generated"]
ignore_vars = ["CI"]
```

## 🤖 CI / GitHub Actions

```yaml
- name: Validate environment files
  run: |
    envex sync --check --all
    envex lint .env.example
    envex scan --check
```

Exit codes: `0` – OK · `1` – problem detected · `2` – runtime error.

## 🪝 Pre-commit hook

```sh
envex hook install     # install
envex hook status      # inspect
envex hook uninstall   # remove
```

On every commit the hook runs `envex sync --all --stage`, so an up-to-date `.env.example` always lands in the repository.

## 🛠️ Development

```sh
cargo test
cargo clippy --all-targets
cargo run -- --help
```

## 📄 License

Apache License 2.0 – see the [LICENSE](LICENSE) file.
