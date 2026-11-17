use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, Ordering};

static COLOR: AtomicBool = AtomicBool::new(false);
static QUIET: AtomicBool = AtomicBool::new(false);

pub fn init(no_color: bool, quiet: bool) {
    let enabled =
        !no_color && std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal();
    COLOR.store(enabled, Ordering::Relaxed);
    QUIET.store(quiet, Ordering::Relaxed);
}

pub fn is_quiet() -> bool {
    QUIET.load(Ordering::Relaxed)
}

fn paint(code: &str, s: &str) -> String {
    if COLOR.load(Ordering::Relaxed) {
        format!("\x1b[{code}m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

pub fn green(s: &str) -> String {
    paint("32", s)
}
pub fn red(s: &str) -> String {
    paint("31", s)
}
pub fn yellow(s: &str) -> String {
    paint("33", s)
}
pub fn cyan(s: &str) -> String {
    paint("36", s)
}
pub fn dim(s: &str) -> String {
    paint("2", s)
}
pub fn bold(s: &str) -> String {
    paint("1", s)
}

pub fn say(msg: impl AsRef<str>) {
    if !is_quiet() {
        println!("{}", msg.as_ref());
    }
}

pub fn success(msg: impl AsRef<str>) {
    say(format!("{} {}", green("✔"), msg.as_ref()));
}

pub fn warn(msg: impl AsRef<str>) {
    if !is_quiet() {
        eprintln!("{} {}", yellow("!"), msg.as_ref());
    }
}

pub fn error(msg: impl AsRef<str>) {
    eprintln!("{} {}", red("✘"), msg.as_ref());
}
