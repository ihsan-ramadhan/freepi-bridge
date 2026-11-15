use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::bridge::{build_state, serve};
use crate::state::{
    clear_state, force_kill, home_dir, is_loopback, log_path, pid_alive, read_state, state_dir,
    terminate, write_state, DEFAULT_PORT, SPACE_PORT,
};
use crate::config;
use crate::update;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Default)]
struct Flags {
    port: Option<u16>,
    bind: Option<String>,
    bg: bool,
    json: bool,
    insecure: bool,
    follow: bool,
    check: bool,
    tag: Option<String>,
    init: bool,
    positionals: Vec<String>,
}

fn help() {
    println!(
        "freepi-bridge {VERSION} - OpenAI-compatible bridge for FreePi

Usage:
  freepi-bridge <command> [flags]

Commands:
  start       run the bridge (--bg = detached)
  stop        stop the background bridge
  restart     stop, then start --bg again
  status      daemon status (--json for machines)
  models      list the models FreePi serves (--json)
  doctor      check token, upstream, bridge and bind safety
  logs        print the bridge log (-f to follow)
  token       print the FreePi JWT the bridge uses
  ads         print the ad page URL
  config      show the config file (--init = create it if missing)
  update      install the latest release (--check = report only)
  version     print the version

Flags:
  --port N          listen port (default 8787)
  --bind H          listen address (default 127.0.0.1)
  --bg              detach after starting
  --json            machine-readable output (status, models, doctor)
  --insecure        allow a non-loopback bind without BRIDGE_API_KEY
  --check           only report whether a newer release exists (update)
  --tag TAG         install a specific release (update, e.g. v0.1.2)
  --init            create the config file if it is missing (config)
  -f, --follow      follow the log
  -h, --help        this help; \"freepi-bridge <command> --help\" per command
  -V, --version     print the version

Environment:
  FREEPI_BASE_URL       upstream API (default https://api.freepi.ai)
  FREEPI_TOKEN          account JWT; falls back to the credentials file
  FREEPI_CREDENTIALS    token file (default ~/.free-pi/agent/credentials.json)
  BRIDGE_API_KEY        shared key clients must send
  HOST / PORT           listen address and port
  FREEPI_BRIDGE_DIR     state and log directory (default ~/.freepi-bridge)
  FREEPI_AUTO_UPDATE    update check on \"start --bg\"; 1 installs, 0 never checks
  FREEPI_BRIDGE_DOWNLOAD_BASE  mirror for release downloads
  TZ                    timezone sent with ad requests

Defaults:
  port {DEFAULT_PORT}, bind 127.0.0.1, state in ~/.freepi-bridge/,
  log in ~/.freepi-bridge/bridge.log
  settings in ~/.freepi-bridge/config.env; exported variables win over it

Exit codes: 0 success, 1 error (daemon down, bad flags, failed check).
"
    );
}

fn command_help(command: &str) -> bool {
    let text = match command {
        "start" => "freepi-bridge start [--bg] [--port N] [--bind H] [--insecure]",
        "stop" => "freepi-bridge stop",
        "restart" => "freepi-bridge restart [--port N] [--bind H]",
        "status" => "freepi-bridge status [--json]",
        "models" => "freepi-bridge models [--json]",
        "doctor" => "freepi-bridge doctor [--json]",
        "logs" => "freepi-bridge logs [-f]",
        "token" => "freepi-bridge token",
        "ads" => "freepi-bridge ads",
        "config" => "freepi-bridge config [--init]",
        "update" => "freepi-bridge update [--check] [--tag vX.Y.Z]",
        "version" => "freepi-bridge version",
        _ => return false,
    };
    println!("{text}");
    true
}

fn parse(argv: Vec<String>) -> Result<Flags, String> {
    let mut flags = Flags::default();
    let mut index = 0;
    while index < argv.len() {
        let arg = argv[index].as_str();
        match arg {
            "--bg" => flags.bg = true,
            "--json" => flags.json = true,
            "--insecure" => flags.insecure = true,
            "--follow" | "-f" => flags.follow = true,
            "--check" => flags.check = true,
            "--init" => flags.init = true,
            "--tag" => {
                index += 1;
                let value = argv.get(index).ok_or("--tag needs a release tag, e.g. v0.1.2")?;
                flags.tag = Some(value.clone());
            }
            "--port" => {
                index += 1;
                let value = argv.get(index).ok_or("--port needs a value")?;
                flags.port = Some(value.parse().map_err(|_| format!("invalid port: {value}"))?);
            }
            "--bind" | "--host" => {
                index += 1;
                flags.bind = Some(argv.get(index).ok_or("--bind needs a value")?.clone());
            }
            other if other.starts_with('-') => return Err(format!("unknown flag: {other}")),
            other => flags.positionals.push(other.to_string()),
        }
        index += 1;
    }
    Ok(flags)
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|value| !value.is_empty())
}

fn default_host() -> String {
    if let Some(host) = env_nonempty("HOST") {
        return host;
    }
    if std::env::var("SPACE_ID").is_ok() {
        "0.0.0.0".to_string()
    } else {
        "127.0.0.1".to_string()
    }
}

fn default_port() -> u16 {
    env_nonempty("PORT")
        .and_then(|value| value.parse().ok())
        .unwrap_or(if std::env::var("SPACE_ID").is_ok() { SPACE_PORT } else { DEFAULT_PORT })
}

fn host_url(host: &str, port: u16) -> String {
    let normalized = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    format!("http://{normalized}:{port}")
}

fn credentials_path() -> PathBuf {
    if let Some(path) = env_nonempty("FREEPI_CREDENTIALS") {
        return PathBuf::from(path);
    }
    home_dir().join(".free-pi/agent/credentials.json")
}

fn resolve_token() -> Option<String> {
    if let Some(token) = env_nonempty("FREEPI_TOKEN") {
        return Some(token);
    }
    let raw = fs::read_to_string(credentials_path()).ok()?;
    let parsed: Value = serde_json::from_str(&raw).ok()?;
    parsed
        .get("token")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
}

fn resolve_base_url() -> String {
    env_nonempty("FREEPI_BASE_URL")
        .unwrap_or_else(|| "https://api.freepi.ai".to_string())
        .trim_end_matches('/')
        .to_string()
}

fn client() -> reqwest::Client {
    crate::bridge::ensure_crypto_provider();
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(5))
        .build()
        .expect("http client")
}

async fn probe_health(host: &str, port: u16) -> bool {
    let probe_host = if host == "0.0.0.0" || host == "::" { "127.0.0.1" } else { host };
    client()
        .get(format!("{}/health", host_url(probe_host, port)))
        .timeout(Duration::from_millis(800))
        .send()
        .await
        .map(|res| res.status().is_success())
        .unwrap_or(false)
}

async fn wait_healthy(host: &str, port: u16) -> bool {
    for _ in 0..40 {
        if probe_health(host, port).await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

async fn bridge_model_count(host: &str, port: u16) -> Option<usize> {
    let mut request = client().get(format!("{}/v1/models", host_url(host, port)));
    if let Some(key) = env_nonempty("BRIDGE_API_KEY") {
        request = request.bearer_auth(key);
    }
    let payload: Value = request.send().await.ok()?.json().await.ok()?;
    Some(payload.get("data")?.as_array()?.len())
}

async fn fetch_catalog() -> Result<(String, Vec<(String, String)>), String> {
    let mut request = client()
        .get(format!("{}/client-version", resolve_base_url()))
        .timeout(Duration::from_secs(8));
    if let Some(token) = resolve_token() {
        request = request.bearer_auth(token);
    }
    let response = request.send().await.map_err(|err| err.to_string())?;
    if !response.status().is_success() {
        return Err(format!("{} /client-version", response.status()));
    }
    let payload: Value = response.json().await.map_err(|err| err.to_string())?;
    let version = payload
        .get("latest")
        .and_then(Value::as_str)
        .or_else(|| payload.get("min").and_then(Value::as_str))
        .unwrap_or("unknown")
        .to_string();
    Ok((version, catalog_models(&payload)))
}

fn catalog_models(payload: &Value) -> Vec<(String, String)> {
    payload
        .get("models")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    let id = entry.get("id")?.as_str()?;
                    let name = entry
                        .get("name")
                        .and_then(Value::as_str)
                        .or_else(|| entry.get("display_name").and_then(Value::as_str))
                        .unwrap_or(id);
                    Some((id.to_string(), name.to_string()))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn guard_bind(host: &str, insecure: bool) -> bool {
    if is_loopback(host) || insecure || env_nonempty("BRIDGE_API_KEY").is_some() {
        return true;
    }
    eprintln!("refusing to bind {host} without BRIDGE_API_KEY: anyone on the network could spend your account");
    eprintln!("set BRIDGE_API_KEY, bind 127.0.0.1, or pass --insecure");
    false
}

async fn cmd_start(flags: Flags) -> i32 {
    if let Err(err) = config::ensure_file() {
        eprintln!("cannot create {}: {err}", config::config_path().display());
    }

    let host = flags.bind.clone().unwrap_or_else(default_host);
    let port = flags.port.unwrap_or_else(default_port);
    let insecure = flags.insecure || env_nonempty("FREEPI_BRIDGE_INSECURE").as_deref() == Some("1");

    if !guard_bind(&host, insecure) {
        return 1;
    }

    if flags.bg {
        let exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(err) => {
                eprintln!("cannot locate the executable: {err}");
                return 1;
            }
        };
        auto_update().await;
        return spawn_detached(&exe, &host, port).await;
    }

    let normalized = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.clone()
    };
    let addr: SocketAddr = match format!("{normalized}:{port}").parse() {
        Ok(addr) => addr,
        Err(_) => {
            eprintln!("invalid address: {host}:{port}");
            return 1;
        }
    };
    match serve(Arc::new(build_state()), addr).await {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("freepi-bridge: {err}");
            1
        }
    }
}

async fn spawn_detached(exe: &Path, host: &str, port: u16) -> i32 {
    if let Some(state) = read_state() {
        if pid_alive(state.pid) {
            println!(
                "already running (pid {}) at {}/v1",
                state.pid,
                host_url(&state.host, state.port)
            );
            return 0;
        }
        clear_state();
    }

    if let Err(err) = fs::create_dir_all(state_dir()) {
        eprintln!("cannot create {}: {err}", state_dir().display());
        return 1;
    }
    let log = match fs::OpenOptions::new().create(true).append(true).open(log_path()) {
        Ok(file) => file,
        Err(err) => {
            eprintln!("cannot open {}: {err}", log_path().display());
            return 1;
        }
    };
    let mut command = Command::new(exe);
    command
        .arg("start")
        .arg("--bind")
        .arg(host)
        .arg("--port")
        .arg(port.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone().expect("log clone")))
        .stderr(Stdio::from(log));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }

    let child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            eprintln!("failed to start: {err}");
            return 1;
        }
    };
    if let Err(err) = write_state(child.id(), host, port) {
        eprintln!("failed to record state: {err}");
        return 1;
    }

    if !wait_healthy(host, port).await {
        eprintln!("started (pid {}) but /health never answered; see {}", child.id(), log_path().display());
        return 1;
    }
    println!("started (pid {}) at {}/v1", child.id(), host_url(host, port));
    0
}

fn stop_daemon() -> i32 {
    let Some(state) = read_state() else {
        println!("not running");
        return 0;
    };
    if !pid_alive(state.pid) {
        clear_state();
        println!("not running");
        return 0;
    }
    terminate(state.pid);
    for _ in 0..50 {
        if !pid_alive(state.pid) {
            clear_state();
            println!("stopped (pid {})", state.pid);
            return 0;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    force_kill(state.pid);
    clear_state();
    if pid_alive(state.pid) {
        eprintln!("pid {} survived SIGKILL", state.pid);
        return 1;
    }
    println!("stopped (pid {})", state.pid);
    0
}

async fn cmd_status(flags: Flags) -> i32 {
    let Some(state) = read_state() else {
        if flags.json {
            println!("{}", json!({ "running": false }));
        } else {
            println!("status: stopped");
        }
        return 1;
    };
    let alive = pid_alive(state.pid);
    let healthy = alive && probe_health(&state.host, state.port).await;
    let models = if healthy {
        bridge_model_count(&state.host, state.port).await
    } else {
        None
    };
    let url = format!("{}/v1", host_url(&state.host, state.port));

    if flags.json {
        println!(
            "{}",
            json!({
                "running": alive,
                "healthy": healthy,
                "pid": state.pid,
                "url": url,
                "models": models,
            })
        );
    } else if healthy {
        let count = models.map(|n| n.to_string()).unwrap_or_else(|| "?".to_string());
        println!("status: running (pid {}) at {url} · {count} models", state.pid);
    } else {
        println!("status: unhealthy (pid {}) at {url}", state.pid);
    }

    if healthy { 0 } else { 1 }
}

async fn cmd_models(flags: Flags) -> i32 {
    match fetch_catalog().await {
        Ok((version, models)) => {
            if flags.json {
                let data: Vec<Value> = models
                    .iter()
                    .map(|(id, name)| json!({ "id": id, "name": name }))
                    .collect();
                println!("{}", serde_json::to_string_pretty(&json!({ "version": version, "data": data })).unwrap_or_default());
            } else {
                println!("FreePi client {version} · {} models", models.len());
                let width = models.iter().map(|(id, _)| id.len()).max().unwrap_or(0).max(4);
                for (id, name) in &models {
                    println!("  {id:<width$}  {name}");
                }
            }
            0
        }
        Err(err) => {
            eprintln!("cannot read the catalog: {err}");
            1
        }
    }
}

fn cmd_token() -> i32 {
    match resolve_token() {
        Some(token) => {
            println!("{token}");
            0
        }
        None => {
            eprintln!(
                "no FreePi token in FREEPI_TOKEN or {}; run `npx free-pi-cli@latest` once to sign in",
                credentials_path().display()
            );
            1
        }
    }
}

fn cmd_ads(flags: Flags) -> i32 {
    let state = read_state();
    let host = flags
        .bind
        .clone()
        .or_else(|| state.as_ref().map(|s| s.host.clone()))
        .unwrap_or_else(default_host);
    let port = flags
        .port
        .or_else(|| state.as_ref().map(|s| s.port))
        .unwrap_or_else(default_port);
    let probe_host = if host == "0.0.0.0" || host == "::" { "127.0.0.1" } else { host.as_str() };
    let mut url = format!("{}/ads", host_url(probe_host, port));
    if let Some(key) = env_nonempty("BRIDGE_API_KEY") {
        url.push_str(&format!("?key={key}"));
    }
    println!("{url}");
    0
}

fn cmd_config(flags: Flags) -> i32 {
    let path = config::config_path();
    match config::ensure_file() {
        Ok(true) => println!("created {}", path.display()),
        Ok(false) => {
            if flags.init {
                println!("config already at {}", path.display());
            } else {
                println!("config {}", path.display());
            }
        }
        Err(err) => {
            eprintln!("cannot create {}: {err}", path.display());
            return 1;
        }
    }
    if flags.init {
        return 0;
    }
    println!();

    match fs::read_to_string(&path) {
        Ok(text) => {
            print!("{text}");
            if !text.ends_with('\n') {
                println!();
            }
            0
        }
        Err(err) => {
            eprintln!("cannot read {}: {err}", path.display());
            1
        }
    }
}

fn cmd_logs(flags: Flags) -> i32 {
    let path = log_path();
    if !path.exists() {
        println!("no log yet at {}", path.display());
        return 0;
    }
    let mut file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(err) => {
            eprintln!("cannot read {}: {err}", path.display());
            return 1;
        }
    };
    let mut contents = String::new();
    if file.read_to_string(&mut contents).is_err() {
        eprintln!("cannot read {}", path.display());
        return 1;
    }
    let lines: Vec<&str> = contents.lines().collect();
    let start = lines.len().saturating_sub(200);
    for line in &lines[start..] {
        println!("{line}");
    }
    if !flags.follow {
        return 0;
    }

    let mut offset = contents.len() as u64;
    loop {
        std::thread::sleep(Duration::from_millis(500));
        if file.seek(SeekFrom::Start(offset)).is_err() {
            return 1;
        }
        let mut chunk = String::new();
        match file.read_to_string(&mut chunk) {
            Ok(0) => {}
            Ok(_) => {
                offset += chunk.len() as u64;
                print!("{chunk}");
                let _ = std::io::stdout().flush();
            }
            Err(_) => return 1,
        }
    }
}

async fn cmd_restart(flags: Flags) -> i32 {
    let code = stop_daemon();
    if code != 0 {
        return code;
    }
    let start_flags = Flags {
        bg: true,
        bind: flags.bind,
        port: flags.port,
        insecure: flags.insecure,
        ..Flags::default()
    };
    cmd_start(start_flags).await
}

async fn auto_update() {
    let mode = update::auto_mode();
    if mode == update::AutoUpdate::Off || !update::check_due() {
        return;
    }
    let Ok(release) = update::latest_release().await else {
        return;
    };
    update::mark_checked();
    if !update::is_newer(VERSION, &release.version) {
        return;
    }

    match mode {
        update::AutoUpdate::Notify => println!(
            "freepi-bridge {} is available (running {VERSION}); run \"freepi-bridge update\"",
            release.tag
        ),
        update::AutoUpdate::Apply => {
            println!("updating freepi-bridge {VERSION} -> {} ...", release.version);
            match update::install(&release).await {
                Ok(exe) => {
                    println!("updated {} to {}", exe.display(), release.version);
                    restart_if_running(&exe).await;
                }
                Err(err) => eprintln!("auto update failed: {err} (still running {VERSION})"),
            }
        }
        update::AutoUpdate::Off => {}
    }
}

async fn restart_if_running(exe: &Path) {
    let Some(state) = read_state() else {
        return;
    };
    if !pid_alive(state.pid) {
        return;
    }
    let insecure = env_nonempty("FREEPI_BRIDGE_INSECURE").as_deref() == Some("1");
    if !guard_bind(&state.host, insecure) {
        println!("daemon left stopped; start it yourself once the bind is allowed");
        return;
    }
    if stop_daemon() != 0 {
        return;
    }
    if spawn_detached(exe, &state.host, state.port).await == 0 {
        println!("daemon restarted on the new build");
    }
}

async fn cmd_update(flags: Flags) -> i32 {
    let release = match flags.tag.as_deref() {
        Some(tag) => {
            let tag = if tag.starts_with('v') || tag.starts_with('V') {
                tag.to_string()
            } else {
                format!("v{tag}")
            };
            if !update::valid_tag(&tag) {
                eprintln!("invalid release tag: {tag}");
                return 1;
            }
            let asset = match update::platform_asset() {
                Ok(asset) => asset,
                Err(err) => {
                    eprintln!("{err}");
                    return 1;
                }
            };
            update::Release {
                version: update::normalize_tag(&tag),
                tag,
                asset,
            }
        }
        None => match update::latest_release().await {
            Ok(release) => release,
            Err(err) => {
                eprintln!("cannot check for updates: {err}");
                return 1;
            }
        },
    };

    let newer = update::is_newer(VERSION, &release.version);
    if flags.check {
        if newer {
            println!("freepi-bridge {} is available (running {VERSION})", release.tag);
        } else if flags.tag.is_some() {
            println!("running freepi-bridge {VERSION}, requested {}", release.tag);
        } else {
            println!("freepi-bridge {VERSION} is up to date (latest {})", release.tag);
        }
        return 0;
    }
    if flags.tag.is_none() && !newer {
        println!("freepi-bridge {VERSION} is up to date (latest {})", release.tag);
        return 0;
    }
    if flags.tag.is_some() && release.version == VERSION {
        println!("freepi-bridge {VERSION} is already installed");
        return 0;
    }

    println!("downloading {} ...", release.asset);
    match update::install(&release).await {
        Ok(exe) => {
            println!(
                "updated {} to {} ({})",
                exe.display(),
                release.version,
                release.tag
            );
            restart_if_running(&exe).await;
            0
        }
        Err(err) => {
            eprintln!("update failed: {err}");
            1
        }
    }
}

fn b64url_decode(input: &str) -> Option<Vec<u8>> {
    let mut accumulator: u32 = 0;
    let mut bits = 0;
    let mut out = Vec::new();
    for byte in input.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            b'=' => break,
            _ => return None,
        };
        accumulator = (accumulator << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((accumulator >> bits) as u8);
        }
    }
    Some(out)
}

fn jwt_expiry(token: &str) -> Option<u64> {
    let payload = token.split('.').nth(1)?;
    let decoded = b64url_decode(payload)?;
    let value: Value = serde_json::from_slice(&decoded).ok()?;
    value.get("exp")?.as_u64()
}

fn civil_date(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!("{year:04}-{m:02}-{d:02}")
}

struct Check {
    ok: bool,
    fatal: bool,
    label: &'static str,
    detail: String,
}

fn check_line(check: &Check) {
    let mark = if check.ok { "✓" } else if check.fatal { "✗" } else { "·" };
    println!("  {mark} {:<22}{}", check.label, check.detail);
}

async fn cmd_doctor(flags: Flags) -> i32 {
    let mut checks: Vec<Check> = Vec::new();

    match resolve_token() {
        Some(token) => {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let (ok, detail) = match jwt_expiry(&token) {
                Some(exp) if exp <= now => (false, format!("expired on {}", civil_date(exp))),
                Some(exp) => (true, format!("ok, expires {}", civil_date(exp))),
                None => (true, "present (expiry unknown)".to_string()),
            };
            checks.push(Check { ok, fatal: !ok, label: "FreePi token", detail });
        }
        None => checks.push(Check {
            ok: false,
            fatal: true,
            label: "FreePi token",
            detail: format!(
                "missing (set FREEPI_TOKEN, or run `npx free-pi-cli@latest` once; checked {})",
                credentials_path().display()
            ),
        }),
    }

    match fetch_catalog().await {
        Ok((version, models)) => checks.push(Check {
            ok: true,
            fatal: false,
            label: "Upstream",
            detail: format!("{} · client {version} · {} models", resolve_base_url(), models.len()),
        }),
        Err(err) => checks.push(Check {
            ok: false,
            fatal: true,
            label: "Upstream",
            detail: format!("{} unreachable ({err})", resolve_base_url()),
        }),
    }

    match read_state() {
        Some(state) => {
            let healthy = pid_alive(state.pid) && probe_health(&state.host, state.port).await;
            let models = if healthy {
                bridge_model_count(&state.host, state.port).await
            } else {
                None
            };
            checks.push(if healthy {
                Check {
                    ok: true,
                    fatal: false,
                    label: "Bridge",
                    detail: format!(
                        "running (pid {}) at {}/v1 · {} models",
                        state.pid,
                        host_url(&state.host, state.port),
                        models.map(|n| n.to_string()).unwrap_or_else(|| "?".into())
                    ),
                }
            } else {
                Check {
                    ok: false,
                    fatal: false,
                    label: "Bridge",
                    detail: format!(
                        "recorded pid {} is not answering; run \"freepi-bridge restart\"",
                        state.pid
                    ),
                }
            });
        }
        None => checks.push(Check {
            ok: false,
            fatal: false,
            label: "Bridge",
            detail: "not started (freepi-bridge start --bg)".to_string(),
        }),
    }

    let bind_host = read_state().map(|s| s.host).unwrap_or_else(default_host);
    let has_key = env_nonempty("BRIDGE_API_KEY").is_some();
    let loopback = is_loopback(&bind_host);
    checks.push(Check {
        ok: loopback || has_key,
        fatal: !loopback && !has_key,
        label: "Security",
        detail: if loopback && !has_key {
            "loopback bind, no key needed".to_string()
        } else if loopback {
            "loopback bind with BRIDGE_API_KEY".to_string()
        } else if has_key {
            format!("bound to {bind_host} with BRIDGE_API_KEY")
        } else {
            format!("bound to {bind_host} without BRIDGE_API_KEY; run start with --insecure to allow")
        },
    });

    if flags.json {
        let payload: Vec<Value> = checks
            .iter()
            .map(|check| {
                json!({ "check": check.label, "ok": check.ok, "detail": check.detail })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json!({ "checks": payload })).unwrap_or_default());
    } else {
        println!("freepi-bridge doctor\n");
        for check in &checks {
            check_line(check);
        }
        println!();
    }

    if checks.iter().any(|check| check.fatal) { 1 } else { 0 }
}

pub(crate) async fn run(argv: Vec<String>) -> i32 {
    update::cleanup_stale();

    let Some(command) = argv.first().cloned() else {
        help();
        return 0;
    };

    if command == "--health" {
        return if probe_health("127.0.0.1", default_port()).await { 0 } else { 1 };
    }
    if command == "--token" {
        return cmd_token();
    }
    if command == "--version" || command == "-V" || command == "version" {
        println!("freepi-bridge {VERSION}");
        return 0;
    }
    if command == "--help" || command == "-h" || command == "help" {
        help();
        return 0;
    }
    if argv.iter().any(|arg| arg == "--help" || arg == "-h") {
        if command_help(&command) {
            return 0;
        }
    }

    let flags = match parse(argv[1..].to_vec()) {
        Ok(flags) => flags,
        Err(err) => {
            eprintln!("freepi-bridge: {err}");
            return 1;
        }
    };

    match command.as_str() {
        "start" => cmd_start(flags).await,
        "stop" => stop_daemon(),
        "restart" => cmd_restart(flags).await,
        "status" => cmd_status(flags).await,
        "models" => cmd_models(flags).await,
        "doctor" => cmd_doctor(flags).await,
        "logs" => cmd_logs(flags),
        "token" => cmd_token(),
        "ads" => cmd_ads(flags),
        "config" => cmd_config(flags),
        "update" => cmd_update(flags).await,
        other => {
            eprintln!("freepi-bridge: unknown command \"{other}\" (try \"freepi-bridge help\")");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jwt_expiry_reads_real_claims() {
        // {"exp":1801899478} base64url
        let token = "h.eyJleHAiOjE4MDE4OTk0Nzh9.sig";
        assert_eq!(jwt_expiry(token), Some(1_801_899_478));
        assert_eq!(jwt_expiry("not-a-jwt"), None);
    }

    #[test]
    fn civil_date_formats_epoch_days() {
        assert_eq!(civil_date(0), "1970-01-01");
        assert_eq!(civil_date(1_801_899_478), "2027-02-06");
    }

    #[test]
    fn parse_rejects_unknown_flags() {
        assert!(parse(vec!["--nope".to_string()]).is_err());
        assert!(parse(vec!["--port".to_string(), "abc".to_string()]).is_err());
        let flags = parse(vec!["--port".to_string(), "9000".to_string(), "--bg".to_string()])
            .expect("valid flags");
        assert_eq!(flags.port, Some(9000));
        assert!(flags.bg);
    }

    #[test]
    fn catalog_keeps_the_marker_that_says_whether_a_model_is_free() {
        let payload = json!({
            "models": [
                { "id": "particle", "name": "Particle · Free Unlimited", "display_name": "Particle" },
                { "id": "solar-pro-4", "name": "Solar Pro 4 · ~0.6x usage", "display_name": "Solar Pro 4" },
                { "id": "bare" }
            ]
        });
        assert_eq!(
            catalog_models(&payload),
            vec![
                ("particle".to_string(), "Particle · Free Unlimited".to_string()),
                ("solar-pro-4".to_string(), "Solar Pro 4 · ~0.6x usage".to_string()),
                ("bare".to_string(), "bare".to_string()),
            ]
        );
    }
}
