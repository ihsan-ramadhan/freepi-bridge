use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;

const DEFAULT_DOWNLOAD_BASE: &str = "https://github.com/ihsan-ramadhan/freepi-bridge/releases";
const LATEST_API: &str = "https://api.github.com/repos/ihsan-ramadhan/freepi-bridge/releases/latest";
const CHECK_INTERVAL_SECS: u64 = 86_400;

pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone)]
pub(crate) struct Release {
    pub(crate) tag: String,
    pub(crate) version: String,
    pub(crate) asset: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutoUpdate {
    Off,
    Notify,
    Apply,
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|value| !value.is_empty())
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

fn binary_name() -> &'static str {
    if cfg!(windows) {
        "freepi-bridge.exe"
    } else {
        "freepi-bridge"
    }
}

pub(crate) fn asset_for(os: &str, arch: &str) -> Option<String> {
    match os {
        "linux" | "macos" => match arch {
            "x86_64" | "aarch64" => Some(format!("freepi-bridge-{os}-{arch}.tar.gz")),
            _ => None,
        },
        "windows" => Some("freepi-bridge-windows-x86_64.zip".to_string()),
        _ => None,
    }
}

pub(crate) fn platform_asset() -> Result<String, String> {
    asset_for(std::env::consts::OS, std::env::consts::ARCH).ok_or_else(|| {
        format!(
            "no prebuilt binary for {} {}; use install.sh, or cargo install --path .",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    })
}

fn download_base() -> String {
    env_nonempty("FREEPI_BRIDGE_DOWNLOAD_BASE")
        .unwrap_or_else(|| DEFAULT_DOWNLOAD_BASE.to_string())
        .trim_end_matches('/')
        .to_string()
}

pub(crate) fn valid_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

pub(crate) fn normalize_tag(tag: &str) -> String {
    tag.trim()
        .trim_start_matches(|c| c == 'v' || c == 'V')
        .to_string()
}

fn version_parts(text: &str) -> Vec<u64> {
    let core = normalize_tag(text);
    let core = core.split(|c: char| c == '-' || c == '+').next().unwrap_or("");
    core.split('.')
        .map_while(|part| part.parse::<u64>().ok())
        .collect()
}

pub(crate) fn is_newer(current: &str, latest: &str) -> bool {
    let mut current = version_parts(current);
    let mut latest = version_parts(latest);
    while current.len() < latest.len() {
        current.push(0);
    }
    while latest.len() < current.len() {
        latest.push(0);
    }
    latest > current
}

const K: [u32; 64] = [
    0x428a_2f98, 0x7137_4491, 0xb5c0_fbcf, 0xe9b5_dba5, 0x3956_c25b, 0x59f1_11f1, 0x923f_82a4,
    0xab1c_5ed5, 0xd807_aa98, 0x1283_5b01, 0x2431_85be, 0x550c_7dc3, 0x72be_5d74, 0x80de_b1fe,
    0x9bdc_06a7, 0xc19b_f174, 0xe49b_69c1, 0xefbe_4786, 0x0fc1_9dc6, 0x240c_a1cc, 0x2de9_2c6f,
    0x4a74_84aa, 0x5cb0_a9dc, 0x76f9_88da, 0x983e_5152, 0xa831_c66d, 0xb003_27c8, 0xbf59_7fc7,
    0xc6e0_0bf3, 0xd5a7_9147, 0x06ca_6351, 0x1429_2967, 0x27b7_0a85, 0x2e1b_2138, 0x4d2c_6dfc,
    0x5338_0d13, 0x650a_7354, 0x766a_0abb, 0x81c2_c92e, 0x9272_2c85, 0xa2bf_e8a1, 0xa81a_664b,
    0xc24b_8b70, 0xc76c_51a3, 0xd192_e819, 0xd699_0624, 0xf40e_3585, 0x106a_a070, 0x19a4_c116,
    0x1e37_6c08, 0x2748_774c, 0x34b0_bcb5, 0x391c_0cb3, 0x4ed8_aa4a, 0x5b9c_ca4f, 0x682e_6ff3,
    0x748f_82ee, 0x78a5_636f, 0x84c8_7814, 0x8cc7_0208, 0x90be_fffa, 0xa450_6ceb, 0xbef9_a3f7,
    0xc671_78f2,
];

fn sha256_hex(data: &[u8]) -> String {
    let mut state: [u32; 8] = [
        0x6a09_e667, 0xbb67_ae85, 0x3c6e_f372, 0xa54f_f53a, 0x510e_527f, 0x9b05_688c, 0x1f83_d9ab,
        0x5be0_cd19,
    ];

    let mut message = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());

    for block in message.chunks_exact(64) {
        let mut schedule = [0u32; 64];
        for (index, word) in block.chunks_exact(4).enumerate() {
            schedule[index] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for index in 16..64 {
            let s0 = schedule[index - 15].rotate_right(7)
                ^ schedule[index - 15].rotate_right(18)
                ^ (schedule[index - 15] >> 3);
            let s1 = schedule[index - 2].rotate_right(17)
                ^ schedule[index - 2].rotate_right(19)
                ^ (schedule[index - 2] >> 10);
            schedule[index] = schedule[index - 16]
                .wrapping_add(s0)
                .wrapping_add(schedule[index - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[index])
                .wrapping_add(schedule[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }

        for (slot, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(value);
        }
    }

    state.iter().map(|word| format!("{word:08x}")).collect()
}

fn client(timeout: Duration) -> reqwest::Client {
    crate::bridge::ensure_crypto_provider();
    reqwest::Client::builder()
        .timeout(timeout)
        .connect_timeout(Duration::from_secs(4))
        .build()
        .expect("http client")
}

fn user_agent() -> String {
    format!("freepi-bridge/{VERSION}")
}

async fn fetch(url: &str, timeout: Duration) -> Result<Vec<u8>, String> {
    let response = client(timeout)
        .get(url)
        .header(reqwest::header::USER_AGENT, user_agent())
        .send()
        .await
        .map_err(|err| format!("cannot download {url}: {err}"))?;
    if !response.status().is_success() {
        return Err(format!("{url}: HTTP {}", response.status()));
    }
    response
        .bytes()
        .await
        .map(|body| body.to_vec())
        .map_err(|err| format!("download of {url} broke off: {err}"))
}

async fn published_checksum(url: &str) -> Result<Option<String>, String> {
    let response = client(Duration::from_secs(15))
        .get(url)
        .header(reqwest::header::USER_AGENT, user_agent())
        .send()
        .await
        .map_err(|err| format!("cannot download {url}: {err}"))?;
    if response.status().as_u16() == 404 {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(format!("{url}: HTTP {}", response.status()));
    }
    let text = response
        .text()
        .await
        .map_err(|err| format!("cannot read {url}: {err}"))?;
    Ok(text.split_whitespace().next().map(str::to_ascii_lowercase))
}

pub(crate) async fn latest_release() -> Result<Release, String> {
    let asset = platform_asset()?;
    let response = client(Duration::from_secs(8))
        .get(LATEST_API)
        .header(reqwest::header::USER_AGENT, user_agent())
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .map_err(|err| format!("cannot reach GitHub: {err}"))?;
    match response.status().as_u16() {
        404 => return Err("no release has been published yet".to_string()),
        code if !(200..300).contains(&code) => return Err(format!("GitHub answered {code}")),
        _ => {}
    }
    let payload: Value = response
        .json()
        .await
        .map_err(|err| format!("unreadable release metadata: {err}"))?;
    let tag = payload
        .get("tag_name")
        .and_then(Value::as_str)
        .filter(|tag| !tag.is_empty())
        .ok_or("release metadata carries no tag_name")?
        .to_string();
    if !valid_tag(&tag) {
        return Err(format!("unexpected release tag: {tag}"));
    }
    if let Some(assets) = payload.get("assets").and_then(Value::as_array) {
        let names: Vec<&str> = assets
            .iter()
            .filter_map(|entry| entry.get("name").and_then(Value::as_str))
            .collect();
        if !names.is_empty() && !names.contains(&asset.as_str()) {
            return Err(format!("release {tag} has no {asset}"));
        }
    }
    Ok(Release {
        version: normalize_tag(&tag),
        tag,
        asset,
    })
}

fn work_dir() -> PathBuf {
    std::env::temp_dir().join(format!("freepi-bridge-update-{}", std::process::id()))
}

fn extract(archive: &Path, dir: &Path) -> Result<(), String> {
    let attempt = Command::new("tar")
        .arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if attempt.map(|status| status.success()).unwrap_or(false) {
        return Ok(());
    }

    #[cfg(windows)]
    {
        let script = format!(
            "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
            archive.display(),
            dir.display()
        );
        let fallback = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if fallback.map(|status| status.success()).unwrap_or(false) {
            return Ok(());
        }
    }

    Err(format!(
        "cannot unpack {} (tar failed or is missing)",
        archive.display()
    ))
}

fn target_exe() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|err| format!("cannot locate this executable: {err}"))
}

#[cfg(unix)]
fn replace_exe(new_binary: &Path) -> Result<PathBuf, String> {
    use std::os::unix::fs::PermissionsExt;

    let exe = target_exe()?;
    let name = exe
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("freepi-bridge");
    let staged = exe.with_file_name(format!(".{name}.{}.new", std::process::id()));
    fs::copy(new_binary, &staged).map_err(|err| {
        format!(
            "cannot write {}: {err} (installed in a protected directory? re-run the installer with sudo)",
            staged.display()
        )
    })?;
    fs::set_permissions(&staged, fs::Permissions::from_mode(0o755))
        .map_err(|err| format!("cannot mark {} executable: {err}", staged.display()))?;
    if let Err(err) = fs::rename(&staged, &exe) {
        let _ = fs::remove_file(&staged);
        return Err(format!("cannot replace {}: {err}", exe.display()));
    }
    Ok(exe)
}

#[cfg(windows)]
fn replace_exe(new_binary: &Path) -> Result<PathBuf, String> {
    let exe = target_exe()?;
    let retired = exe.with_extension("exe.old");
    let _ = fs::remove_file(&retired);
    fs::rename(&exe, &retired).map_err(|err| {
        format!(
            "cannot move {} aside: {err} (is the binary in a protected directory?)",
            exe.display()
        )
    })?;
    if let Err(err) = fs::copy(new_binary, &exe) {
        let _ = fs::rename(&retired, &exe);
        return Err(format!("cannot install {}: {err}", exe.display()));
    }
    let _ = fs::remove_file(&retired);
    Ok(exe)
}

pub(crate) fn cleanup_stale() {
    #[cfg(windows)]
    {
        if let Ok(exe) = std::env::current_exe() {
            let _ = fs::remove_file(exe.with_extension("exe.old"));
        }
    }
}

pub(crate) async fn install(release: &Release) -> Result<PathBuf, String> {
    let url = format!(
        "{}/download/{}/{}",
        download_base(),
        release.tag,
        release.asset
    );
    let archive_bytes = fetch(&url, Duration::from_secs(180)).await?;

    match published_checksum(&format!("{url}.sha256")).await? {
        Some(expected) => {
            let actual = sha256_hex(&archive_bytes);
            if actual != expected {
                return Err(format!(
                    "checksum mismatch for {}: expected {expected}, got {actual}",
                    release.asset
                ));
            }
        }
        None => eprintln!("warning: {} publishes no checksum", url),
    }

    let dir = work_dir();
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).map_err(|err| format!("cannot create {}: {err}", dir.display()))?;

    let result = (|| -> Result<PathBuf, String> {
        let archive = dir.join(&release.asset);
        fs::write(&archive, &archive_bytes)
            .map_err(|err| format!("cannot write {}: {err}", archive.display()))?;
        extract(&archive, &dir)?;
        let unpacked = dir.join(binary_name());
        if !unpacked.exists() {
            return Err(format!("{} contains no {}", release.asset, binary_name()));
        }
        replace_exe(&unpacked)
    })();

    let _ = fs::remove_dir_all(&dir);
    result
}

pub(crate) fn auto_mode() -> AutoUpdate {
    let Some(value) = env_nonempty("FREEPI_AUTO_UPDATE").map(|v| v.to_ascii_lowercase()) else {
        return AutoUpdate::Notify;
    };
    match value.as_str() {
        "0" | "false" | "off" | "no" => AutoUpdate::Off,
        "1" | "true" | "on" | "yes" => AutoUpdate::Apply,
        _ => AutoUpdate::Notify,
    }
}

fn stamp_path() -> PathBuf {
    crate::state::state_dir().join("update-check")
}

pub(crate) fn check_due() -> bool {
    let Ok(raw) = fs::read_to_string(stamp_path()) else {
        return true;
    };
    match raw.trim().parse::<u64>() {
        Ok(last) => now_secs().saturating_sub(last) >= CHECK_INTERVAL_SECS,
        Err(_) => true,
    }
}

pub(crate) fn mark_checked() {
    let dir = crate::state::state_dir();
    let _ = fs::create_dir_all(&dir);
    let _ = fs::write(dir.join("update-check"), now_secs().to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_names_match_the_release_workflow() {
        assert_eq!(
            asset_for("linux", "x86_64").as_deref(),
            Some("freepi-bridge-linux-x86_64.tar.gz")
        );
        assert_eq!(
            asset_for("macos", "aarch64").as_deref(),
            Some("freepi-bridge-macos-aarch64.tar.gz")
        );
        assert_eq!(
            asset_for("windows", "aarch64").as_deref(),
            Some("freepi-bridge-windows-x86_64.zip")
        );
        assert_eq!(asset_for("freebsd", "x86_64"), None);
        assert_eq!(asset_for("linux", "riscv64"), None);
    }

    #[test]
    fn newer_versions_compare_numerically() {
        assert!(is_newer("0.1.1", "0.1.2"));
        assert!(is_newer("0.1.1", "v0.2.0"));
        assert!(is_newer("0.9.0", "0.10.0"));
        assert!(is_newer("0.1", "0.1.1"));
        assert!(!is_newer("0.1.1", "0.1.1"));
        assert!(!is_newer("0.1.1", "v0.1.1"));
        assert!(!is_newer("0.2.0", "0.1.9"));
        assert_eq!(normalize_tag("v0.1.2"), "0.1.2");
        assert_eq!(version_parts("v0.1.2-rc.3"), vec![0, 1, 2]);
    }

    #[test]
    fn tags_are_url_safe() {
        assert!(valid_tag("v0.1.2"));
        assert!(valid_tag("v1.0.0-beta_1"));
        assert!(!valid_tag("../etc/passwd"));
        assert!(!valid_tag(""));
        assert!(!valid_tag("v0.1.2/evil"));
    }

    #[test]
    fn sha256_matches_reference_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }
}
