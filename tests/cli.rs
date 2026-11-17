use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("envex-test-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn envex(dir: &PathBuf, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_envex"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn out(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[test]
fn generate_then_refuses_overwrite_without_force() {
    let d = tmp("gen");
    fs::write(
        d.join(".env"),
        "# db\nURL=postgres://u:p@h/db?x=1\nTOKEN=a==\n",
    )
    .unwrap();
    assert!(envex(&d, &["generate"]).status.success());
    assert_eq!(
        fs::read_to_string(d.join(".env.example")).unwrap(),
        "# db\nURL=\nTOKEN=\n"
    );
    assert!(!envex(&d, &["generate"]).status.success());
    assert!(envex(&d, &["generate", "--force"]).status.success());
}

#[test]
fn sync_check_detects_drift_and_fixes_it() {
    let d = tmp("sync");
    fs::write(d.join(".env"), "A=1\nB=2\n").unwrap();
    fs::write(d.join(".env.example"), "# mine\nA=\n").unwrap();
    assert_eq!(envex(&d, &["sync", "--check"]).status.code(), Some(1));
    assert!(envex(&d, &["sync"]).status.success());
    assert_eq!(
        fs::read_to_string(d.join(".env.example")).unwrap(),
        "# mine\nA=\nB=\n"
    );
    assert_eq!(envex(&d, &["sync", "--check"]).status.code(), Some(0));
}

#[test]
fn check_reports_missing_keys() {
    let d = tmp("check");
    fs::write(d.join(".env"), "A=1\n").unwrap();
    fs::write(d.join(".env.example"), "A=\nB=\n").unwrap();
    let o = envex(&d, &["check", "--no-color"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(out(&o).contains("B"));
}

#[test]
fn lint_flags_leaked_secret_in_example() {
    let d = tmp("lint");
    fs::write(
        d.join(".env.example"),
        "API_TOKEN=ghp_abcdefghijklmnop\nPORT=3000\n",
    )
    .unwrap();
    let o = envex(&d, &["lint", ".env.example"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(out(&o).contains("leak"));
}

#[test]
fn scan_finds_vars_in_code_and_fix_appends() {
    let d = tmp("scan");
    fs::create_dir_all(d.join("src")).unwrap();
    fs::write(d.join("src/app.js"), "const a = process.env.API_URL;\n").unwrap();
    fs::write(
        d.join("src/main.rs"),
        "fn main(){ let _ = std::env::var(\"DB_URL\"); }\n",
    )
    .unwrap();
    fs::write(d.join(".env.example"), "OLD=\n").unwrap();
    assert_eq!(envex(&d, &["scan", "--check"]).status.code(), Some(1));
    assert!(envex(&d, &["scan", "--fix"]).status.success());
    let ex = fs::read_to_string(d.join(".env.example")).unwrap();
    assert!(ex.contains("API_URL=") && ex.contains("DB_URL="));
}

#[test]
fn run_passes_env_to_child() {
    let d = tmp("run");
    fs::write(d.join(".env"), "GREETING=\"hello world\"\n").unwrap();
    let o = envex(&d, &["run", "--", "sh", "-c", "echo $GREETING"]);
    assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "hello world");
}

#[test]
fn fill_creates_env_from_example() {
    let d = tmp("fill");
    fs::write(d.join(".env.example"), "# c\nPORT=3000\nJWT_SECRET=\n").unwrap();
    assert!(
        envex(&d, &["fill", "--yes", "--random-secrets"])
            .status
            .success()
    );
    let env = fs::read_to_string(d.join(".env")).unwrap();
    assert!(env.contains("PORT=3000"));
    let secret = env.lines().find(|l| l.starts_with("JWT_SECRET=")).unwrap();
    assert_eq!(secret.len(), "JWT_SECRET=".len() + 64);
}

#[test]
fn export_shell_quotes_values() {
    let d = tmp("export");
    fs::write(d.join(".env"), "A=\"it's ok\"\n").unwrap();
    let o = envex(&d, &["export", "-f", "shell"]);
    assert_eq!(
        String::from_utf8_lossy(&o.stdout).trim(),
        "export A='it'\\''s ok'"
    );
}
