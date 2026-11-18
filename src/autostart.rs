use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::state::home_dir;
#[cfg(target_os = "macos")]
use crate::state::log_path;

#[cfg(target_os = "macos")]
const LABEL: &str = "dev.freepi.bridge";
#[cfg(windows)]
const TASK: &str = "freepi-bridge";

fn run(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|err| format!("cannot run {program}: {err}"))?;
    let mut text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        let detail = if stderr.trim().is_empty() { text } else { stderr.trim().to_string() };
        return Err(format!("{program} failed: {detail}"));
    }
    if text.is_empty() {
        text = stderr.trim().to_string();
    }
    Ok(text)
}

fn quiet(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn exe_path() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|err| format!("cannot locate the executable: {err}"))
}

fn daemon_command(exe: &Path, host: &str, port: u16, insecure: bool) -> String {
    let flag = if insecure { " --insecure" } else { "" };
    format!("\"{}\" start --bind {host} --port {port}{flag}", exe.display())
}

#[cfg(target_os = "macos")]
fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(target_os = "linux")]
fn unit_path() -> PathBuf {
    home_dir().join(".config/systemd/user/freepi-bridge.service")
}

#[cfg(target_os = "linux")]
pub(crate) fn unit_file(exe: &Path, host: &str, port: u16, insecure: bool) -> String {
    format!(
        "[Unit]\n\
         Description=freepi-bridge, OpenAI-compatible bridge for FreePi\n\
         After=network-online.target\n\
         Wants=network-online.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={}\n\
         Environment=FREEPI_BRIDGE_NO_AUTO_UPDATE=1\n\
         Restart=always\n\
         RestartSec=3\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        daemon_command(exe, host, port, insecure)
    )
}

#[cfg(target_os = "macos")]
fn plist_path() -> PathBuf {
    home_dir().join("Library/LaunchAgents/dev.freepi.bridge.plist")
}

#[cfg(target_os = "macos")]
pub(crate) fn plist(exe: &Path, host: &str, port: u16, insecure: bool) -> String {
    let log = log_path();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
        <string>start</string>
        <string>--bind</string>
        <string>{}</string>
        <string>--port</string>
        <string>{}</string>{}
    </array>
    <key>EnvironmentVariables</key>
    <dict>
        <key>FREEPI_BRIDGE_NO_AUTO_UPDATE</key>
        <string>1</string>
    </dict>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{}</string>
    <key>StandardErrorPath</key>
    <string>{}</string>
</dict>
</plist>
"#,
        xml_escape(&exe.display().to_string()),
        xml_escape(host),
        port,
        if insecure { "\n        <string>--insecure</string>" } else { "" },
        xml_escape(&log.display().to_string()),
        xml_escape(&log.display().to_string()),
    )
}

#[cfg(windows)]
pub(crate) fn task_command(exe: &Path, host: &str, port: u16, insecure: bool) -> String {
    let flag = if insecure { " --insecure" } else { "" };
    format!(
        "cmd /c set FREEPI_BRIDGE_NO_AUTO_UPDATE=1&& \"{}\" start --bind {host} --port {port}{flag}",
        exe.display()
    )
}

pub(crate) fn installed() -> bool {
    #[cfg(target_os = "linux")]
    {
        unit_path().exists()
    }
    #[cfg(target_os = "macos")]
    {
        plist_path().exists()
    }
    #[cfg(windows)]
    {
        quiet("schtasks", &["/Query", "/TN", TASK])
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        false
    }
}

pub(crate) fn active() -> bool {
    #[cfg(target_os = "linux")]
    {
        installed() && quiet("systemctl", &["--user", "is-active", "--quiet", "freepi-bridge.service"])
    }
    #[cfg(target_os = "macos")]
    {
        installed() && quiet("launchctl", &["print", &format!("gui/{}/{}", uid(), LABEL)])
    }
    #[cfg(windows)]
    {
        installed()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        false
    }
}

#[cfg(target_os = "macos")]
fn uid() -> u32 {
    unsafe extern "C" {
        fn getuid() -> u32;
    }
    unsafe { getuid() }
}

pub(crate) fn enable(host: &str, port: u16, insecure: bool) -> Result<String, String> {
    let exe = exe_path()?;

    #[cfg(target_os = "linux")]
    {
        let path = unit_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| format!("cannot create {}: {err}", parent.display()))?;
        }
        fs::write(&path, unit_file(&exe, host, port, insecure))
            .map_err(|err| format!("cannot write {}: {err}", path.display()))?;
        run("systemctl", &["--user", "daemon-reload"])?;
        if run("systemctl", &["--user", "enable", "--now", "freepi-bridge.service"]).is_err() {
            let _ = fs::remove_file(&path);
            let _ = run("systemctl", &["--user", "daemon-reload"]);
            return Err(format!(
                "the unit was written but could not start (no user session or dbus?); removed {}",
                path.display()
            ));
        }

        let user = std::env::var("USER")
            .or_else(|_| std::env::var("LOGNAME"))
            .unwrap_or_default();
        if user.is_empty() || !quiet("loginctl", &["enable-linger", &user]) {
            return Ok(format!(
                "enabled for your login session; run `loginctl enable-linger $USER` once to start it at boot without logging in ({})",
                path.display()
            ));
        }
        Ok(format!("enabled at boot and login ({})", path.display()))
    }

    #[cfg(target_os = "macos")]
    {
        let path = plist_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| format!("cannot create {}: {err}", parent.display()))?;
        }
        fs::write(&path, plist(&exe, host, port, insecure))
            .map_err(|err| format!("cannot write {}: {err}", path.display()))?;
        let domain = format!("gui/{}", uid());
        let _ = run("launchctl", &["bootout", &format!("{domain}/{LABEL}")]);
        if run("launchctl", &["bootstrap", &domain, &path.display().to_string()]).is_err() {
            let _ = fs::remove_file(&path);
            return Err(format!("launchctl could not load the agent; removed {}", path.display()));
        }
        Ok(format!("enabled at login ({})", path.display()))
    }

    #[cfg(windows)]
    {
        let _ = run("schtasks", &["/Delete", "/TN", TASK, "/F"]);
        run(
            "schtasks",
            &[
                "/Create",
                "/TN",
                TASK,
                "/SC",
                "ONLOGON",
                "/RL",
                "LIMITED",
                "/TR",
                &task_command(&exe, host, port, insecure),
                "/F",
            ],
        )?;
        if !quiet("schtasks", &["/Run", "/TN", TASK]) {
            let _ = run("schtasks", &["/Delete", "/TN", TASK, "/F"]);
            return Err("the task was created but could not start; removed it".to_string());
        }
        Ok("enabled at login (scheduled task freepi-bridge)".to_string())
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = (host, port, exe);
        Err("autostart is only implemented for Linux, macOS and Windows".to_string())
    }
}

pub(crate) fn disable() -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        let path = unit_path();
        let _ = run("systemctl", &["--user", "disable", "--now", "freepi-bridge.service"]);
        if path.exists() {
            fs::remove_file(&path).map_err(|err| format!("cannot remove {}: {err}", path.display()))?;
        }
        let _ = run("systemctl", &["--user", "daemon-reload"]);
        Ok("autostart disabled".to_string())
    }
    #[cfg(target_os = "macos")]
    {
        let path = plist_path();
        let domain = format!("gui/{}", uid());
        let _ = run("launchctl", &["bootout", &format!("{domain}/{LABEL}")]);
        if path.exists() {
            fs::remove_file(&path).map_err(|err| format!("cannot remove {}: {err}", path.display()))?;
        }
        Ok("autostart disabled".to_string())
    }
    #[cfg(windows)]
    {
        let _ = run("schtasks", &["/End", "/TN", TASK]);
        if installed() {
            run("schtasks", &["/Delete", "/TN", TASK, "/F"])?;
        }
        Ok("autostart disabled".to_string())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        Err("autostart is only implemented for Linux, macOS and Windows".to_string())
    }
}

pub(crate) fn restart_service() -> bool {
    #[cfg(target_os = "linux")]
    {
        active() && quiet("systemctl", &["--user", "restart", "freepi-bridge.service"])
    }
    #[cfg(target_os = "macos")]
    {
        active()
            && quiet(
                "launchctl",
                &["kickstart", "-k", &format!("gui/{}/{}", uid(), LABEL)],
            )
    }
    #[cfg(windows)]
    {
        if !installed() {
            return false;
        }
        let _ = quiet("schtasks", &["/End", "/TN", TASK]);
        quiet("schtasks", &["/Run", "/TN", TASK])
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn xml_escape_only_touches_markup_characters() {
        assert_eq!(xml_escape("a&b<c>d\"e"), "a&amp;b&lt;c&gt;d&quot;e");
        assert_eq!(xml_escape("/home/plain/freepi-bridge"), "/home/plain/freepi-bridge");
    }

    #[test]
    fn daemon_command_carries_bind_and_port() {
        let command = daemon_command(Path::new("/usr/local/bin/freepi-bridge"), "127.0.0.1", 8787, false);
        assert_eq!(
            command,
            "\"/usr/local/bin/freepi-bridge\" start --bind 127.0.0.1 --port 8787"
        );
        let spaced = daemon_command(Path::new("/home/u/my bin/freepi-bridge"), "127.0.0.1", 1, false);
        assert!(spaced.starts_with("\"/home/u/my bin/freepi-bridge\" "));
        let open = daemon_command(Path::new("/bin/fpb"), "0.0.0.0", 8787, true);
        assert!(open.ends_with(" --insecure"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn unit_restarts_the_daemon_and_opts_out_of_self_update() {
        let unit = unit_file(Path::new("/home/u/.local/bin/freepi-bridge"), "127.0.0.1", 8787, false);
        assert!(unit.contains(
            "ExecStart=\"/home/u/.local/bin/freepi-bridge\" start --bind 127.0.0.1 --port 8787"
        ));
        assert!(unit.contains("Restart=always"));
        assert!(unit.contains("FREEPI_BRIDGE_NO_AUTO_UPDATE=1"));
        assert!(unit.contains("WantedBy=default.target"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn plist_keeps_the_daemon_alive() {
        let text = plist(Path::new("/Users/u/bin/freepi-bridge"), "0.0.0.0", 8787, true);
        assert!(text.contains("<string>--insecure</string>"));
        assert!(text.contains("<key>RunAtLoad</key>\n    <true/>"));
        assert!(text.contains("<key>KeepAlive</key>\n    <true/>"));
        assert!(text.contains("<string>--port</string>"));
        assert!(text.contains("<string>8787</string>"));
    }

    #[cfg(windows)]
    #[test]
    fn task_command_quotes_the_executable() {
        let command = task_command(Path::new(r"C:\Users\u\freepi-bridge.exe"), "127.0.0.1", 8787, false);
        assert_eq!(command, r#""C:\Users\u\freepi-bridge.exe" start --bind 127.0.0.1 --port 8787"#);
    }
}
