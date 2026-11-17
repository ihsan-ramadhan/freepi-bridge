use std::fs;
use std::io::Write;
use std::path::PathBuf;

use crate::state::state_dir;

const KNOWN: [&str; 14] = [
    "FREEPI_BASE_URL",
    "FREEPI_TOKEN",
    "FREEPI_CREDENTIALS",
    "BRIDGE_API_KEY",
    "HOST",
    "PORT",
    "FREEPI_BRIDGE_INSECURE",
    "FREEPI_AUTO_AD",
    "FREEPI_AD_REFRESH_SECS",
    "FREEPI_MAX_INFLIGHT",
    "FREEPI_QUEUE_WAIT_SECS",
    "FREEPI_AUTO_UPDATE",
    "FREEPI_BRIDGE_DOWNLOAD_BASE",
    "TZ",
];

pub(crate) const TEMPLATE: &str = r#"# freepi-bridge configuration
#
# Read at every start, so the daemon keeps these settings after a reboot
# without exports in a shell profile. A variable already exported in the
# environment wins over this file, which keeps one-off overrides working:
#   FREEPI_AUTO_AD=0 freepi-bridge start --bg
#
# Apply edits with: freepi-bridge restart
# Show this file:   freepi-bridge config

# Report ads from the bridge itself, so hosts that cannot open /ads
# (container, CI runner) still get completions. 1 = on, 0 = off.
# Reporting an ad nobody saw can get the account reviewed, so set 0 wherever a
# browser can keep the ad page open.
FREEPI_AUTO_AD=1

# Seconds between automatic ad reports; values under 30 are ignored.
FREEPI_AD_REFRESH_SECS=600

# FreePi serves one completion per account. A second request waits for the slot
# instead of failing with "429 concurrent"; 0 disables the gate and lets
# everything through at once.
# FREEPI_MAX_INFLIGHT=1
# FREEPI_QUEUE_WAIT_SECS=30

# Shared key clients must send as "authorization: Bearer ...". Empty = no key.
BRIDGE_API_KEY=

# Listen address. Keep the loopback default unless BRIDGE_API_KEY is set.
# HOST=127.0.0.1
# PORT=8787

# Account JWT; empty means "use the credentials file written by the FreePi CLI".
# FREEPI_TOKEN=
# FREEPI_BASE_URL=https://api.freepi.ai

# Safety valve for a non-loopback bind without BRIDGE_API_KEY.
# FREEPI_BRIDGE_INSECURE=0

# Update check on "start --bg": 1 installs, 0 never checks, empty = notify only.
# FREEPI_AUTO_UPDATE=

# Mirror for update downloads.
# FREEPI_BRIDGE_DOWNLOAD_BASE=https://github.com/ihsan-ramadhan/freepi-bridge/releases

# Timezone sent with ad requests.
# TZ=UTC
"#;

pub(crate) fn config_path() -> PathBuf {
    state_dir().join("config.env")
}

pub(crate) fn ensure_file() -> Result<bool, String> {
    let path = config_path();
    if path.exists() {
        return Ok(false);
    }
    fs::create_dir_all(state_dir())
        .map_err(|err| format!("cannot create {}: {err}", state_dir().display()))?;

    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&path)
        .map_err(|err| format!("cannot create {}: {err}", path.display()))?;
    file.write_all(TEMPLATE.as_bytes())
        .map_err(|err| format!("cannot write {}: {err}", path.display()))?;
    Ok(true)
}

pub(crate) fn apply() {
    let path = config_path();
    let Ok(text) = fs::read_to_string(&path) else {
        return;
    };
    let parsed = parse(&text);
    for warning in &parsed.warnings {
        eprintln!("config {}: {warning}", path.display());
    }
    for (key, value) in parsed.entries {
        if std::env::var_os(&key).is_some() {
            continue;
        }
        std::env::set_var(&key, value);
    }
}

struct Parsed {
    entries: Vec<(String, String)>,
    warnings: Vec<String>,
}

fn parse(text: &str) -> Parsed {
    let mut entries: Vec<(String, String)> = Vec::new();
    let mut warnings = Vec::new();

    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line_number = index + 1;
        let Some((key, value)) = line.split_once('=') else {
            warnings.push(format!("line {line_number}: expected KEY=VALUE, got {line:?}"));
            continue;
        };
        let key = key.trim();
        if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            warnings.push(format!("line {line_number}: invalid key {key:?}"));
            continue;
        }
        if !KNOWN.contains(&key) {
            warnings.push(format!("line {line_number}: unknown key {key}"));
            continue;
        }
        let value = parse_value(value);
        if value.contains('\0') {
            warnings.push(format!("line {line_number}: {key} contains a NUL byte"));
            continue;
        }
        entries.retain(|(existing, _)| existing != key);
        entries.push((key.to_string(), value));
    }

    Parsed { entries, warnings }
}

fn parse_value(raw: &str) -> String {
    let raw = raw.trim();
    for quote in ['"', '\''] {
        if let Some(rest) = raw.strip_prefix(quote) {
            if let Some(end) = rest.rfind(quote) {
                return rest[..end].to_string();
            }
        }
    }
    let mut cut = raw.len();
    for (index, c) in raw.char_indices() {
        if c == '#' && index > 0 {
            let before = &raw[..index];
            if before.ends_with(' ') || before.ends_with('\t') {
                cut = index;
                break;
            }
        }
    }
    raw[..cut].trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_comments_blank_lines_and_whitespace() {
        let parsed = parse(
            "# comment\n\n  FREEPI_AUTO_AD = 1  \n\tFREEPI_AD_REFRESH_SECS=300\n#PORT=1\n",
        );
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
        assert_eq!(
            parsed.entries,
            vec![
                ("FREEPI_AUTO_AD".to_string(), "1".to_string()),
                ("FREEPI_AD_REFRESH_SECS".to_string(), "300".to_string()),
            ]
        );
    }

    #[test]
    fn strips_matching_quotes_only() {
        let parsed = parse("BRIDGE_API_KEY=\"s3cr#t\"\nFREEPI_TOKEN='abc'\nTZ=\"\"\n");
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
        assert_eq!(
            parsed.entries,
            vec![
                ("BRIDGE_API_KEY".to_string(), "s3cr#t".to_string()),
                ("FREEPI_TOKEN".to_string(), "abc".to_string()),
                ("TZ".to_string(), String::new()),
            ]
        );
        assert_eq!(parse_value("\"half"), "\"half");
    }

    #[test]
    fn inline_comments_need_a_space_before_the_hash() {
        let parsed = parse(
            "PORT=8787 # default\nBRIDGE_API_KEY=abc#def\nFREEPI_TOKEN=\"x # y\" # note\n",
        );
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
        assert_eq!(
            parsed.entries,
            vec![
                ("PORT".to_string(), "8787".to_string()),
                ("BRIDGE_API_KEY".to_string(), "abc#def".to_string()),
                ("FREEPI_TOKEN".to_string(), "x # y".to_string()),
            ]
        );
    }

    #[test]
    fn reports_unknown_and_malformed_lines() {
        let parsed = parse("FREEPI_AUTO_ADS=1\nnonsense\n=1\nFREEPI_AUTO_AD=1\n");
        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.entries[0].0, "FREEPI_AUTO_AD");
        assert_eq!(parsed.warnings.len(), 3, "{:?}", parsed.warnings);
        assert!(parsed.warnings[0].contains("unknown key FREEPI_AUTO_ADS"));
        assert!(parsed.warnings[1].contains("expected KEY=VALUE"));
        assert!(parsed.warnings[2].contains("invalid key"));
    }

    #[test]
    fn last_duplicate_wins() {
        let parsed = parse("PORT=8787\nPORT=9000\n");
        assert_eq!(
            parsed.entries,
            vec![("PORT".to_string(), "9000".to_string())]
        );
    }

    #[test]
    fn shipped_template_is_valid() {
        let parsed = parse(TEMPLATE);
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
        assert!(parsed
            .entries
            .iter()
            .any(|(key, value)| key == "FREEPI_AUTO_AD" && value == "1"));
    }

    #[test]
    fn bridge_dir_cannot_be_set_from_the_file() {
        assert!(!KNOWN.contains(&"FREEPI_BRIDGE_DIR"));
    }
}
