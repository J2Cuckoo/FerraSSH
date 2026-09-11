//! Daily rolling operation logs next to the installed executable.

use chrono::{Local, NaiveDate};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Mutex, OnceLock};
use std::thread;

enum Job {
    Line(String),
    Prune(u32),
}

fn sender() -> mpsc::Sender<Job> {
    static TX: OnceLock<Mutex<mpsc::Sender<Job>>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Job>();
        thread::Builder::new()
            .name("ferrassh-oplog".into())
            .spawn(move || worker(rx))
            .ok();
        Mutex::new(tx)
    })
    .lock()
    .unwrap_or_else(|p| p.into_inner())
    .clone()
}

fn worker(rx: mpsc::Receiver<Job>) {
    for job in rx {
        match job {
            Job::Line(line) => {
                let _ = append_today(&line);
            }
            Job::Prune(days) => {
                let _ = prune(days);
            }
        }
    }
}

pub fn install_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn logs_dir() -> PathBuf {
    install_dir().join("logs")
}

fn today_file() -> PathBuf {
    logs_dir().join(format!("ferrassh-{}.log", Local::now().format("%Y-%m-%d")))
}

fn append_today(line: &str) -> std::io::Result<()> {
    let dir = logs_dir();
    fs::create_dir_all(&dir)?;
    let mut f = OpenOptions::new().create(true).append(true).open(today_file())?;
    writeln!(f, "{line}")
}

fn parse_log_date(name: &str) -> Option<NaiveDate> {
    let rest = name.strip_prefix("ferrassh-")?.strip_suffix(".log")?;
    NaiveDate::parse_from_str(rest, "%Y-%m-%d").ok()
}

fn prune(days: u32) -> std::io::Result<()> {
    let dir = logs_dir();
    if !dir.exists() {
        return Ok(());
    }
    let keep = i64::from(days.max(1));
    let today = Local::now().date_naive();
    for ent in fs::read_dir(dir)? {
        let ent = ent?;
        let name = ent.file_name();
        let name = name.to_string_lossy();
        let Some(date) = parse_log_date(&name) else {
            continue;
        };
        if today.signed_duration_since(date).num_days() >= keep {
            let _ = fs::remove_file(ent.path());
        }
    }
    Ok(())
}

/// `actor` is `human` or `ai`.
pub fn write(actor: &str, action: &str, detail: &str) {
    let ts = Local::now().format("%Y-%m-%d %H:%M:%S");
    let actor = if actor.eq_ignore_ascii_case("ai") { "ai" } else { "human" };
    let line = format!("{ts} [{actor}] {action} {detail}");
    let _ = sender().send(Job::Line(line));
}

pub fn prune_async(days: u32) {
    let _ = sender().send(Job::Prune(days.max(1)));
}

pub fn logs_dir_display() -> String {
    logs_dir().to_string_lossy().into_owned()
}

pub fn is_log_path(path: &Path) -> bool {
    path.starts_with(logs_dir())
}
