use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Command;

use serde_json::{json, Value};

pub(crate) fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|value| !value.is_empty())
}

pub(crate) const DEFAULT_PORT: u16 = 8787;
pub(crate) const SPACE_PORT: u16 = 7860;

#[derive(Debug)]
pub(crate) struct DaemonState {
    pub pid: u32,
    pub host: String,
    pub port: u16,
}

pub(crate) fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| std::env::var("USERPROFILE").ok().filter(|value| !value.is_empty()))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub(crate) fn state_dir() -> PathBuf {
    match std::env::var("FREEPI_BRIDGE_DIR").ok().filter(|v| !v.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => home_dir().join(".freepi-bridge"),
    }
}

pub(crate) fn state_path() -> PathBuf {
    state_dir().join("state.json")
}

pub(crate) fn log_path() -> PathBuf {
    state_dir().join("bridge.log")
}

pub(crate) fn read_state() -> Option<DaemonState> {
    let raw = fs::read_to_string(state_path()).ok()?;
    let parsed: Value = serde_json::from_str(&raw).ok()?;
    Some(DaemonState {
        pid: parsed.get("pid")?.as_u64()? as u32,
        host: parsed.get("host")?.as_str()?.to_string(),
        port: parsed.get("port")?.as_u64()? as u16,
    })
}

pub(crate) fn write_state(pid: u32, host: &str, port: u16) -> io::Result<()> {
    fs::create_dir_all(state_dir())?;
    fs::write(state_path(), json!({ "pid": pid, "host": host, "port": port }).to_string())
}

pub(crate) fn clear_state() {
    let _ = fs::remove_file(state_path());
}

#[cfg(unix)]
pub(crate) fn pid_alive(pid: u32) -> bool {
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(windows)]
pub(crate) fn pid_alive(pid: u32) -> bool {
    let Ok(output) = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()
    else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let expected = pid.to_string();
    String::from_utf8_lossy(&output.stdout).lines().any(|line| {
        line.split(',')
            .nth(1)
            .map(|field| field.trim().trim_matches('"') == expected)
            .unwrap_or(false)
    })
}

#[cfg(unix)]
pub(crate) fn terminate(pid: u32) -> bool {
    Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(windows)]
pub(crate) fn terminate(pid: u32) -> bool {
    Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T"])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(unix)]
pub(crate) fn force_kill(pid: u32) -> bool {
    Command::new("kill")
        .arg("-KILL")
        .arg(pid.to_string())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(windows)]
pub(crate) fn force_kill(pid: u32) -> bool {
    Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

pub(crate) fn is_loopback(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1" | "[::1]")
}
