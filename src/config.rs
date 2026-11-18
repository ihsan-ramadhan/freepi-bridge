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
#   FREEPI_AUTO_AD=off freepi-bridge start --bg
#
# Apply edits with: freepi-bridge restart
# Show this file:   freepi-bridge config
# Change one line:  freepi-bridge config --set FREEPI_AUTO_UPDATE=off

# Report ads from the bridge itself, so hosts that cannot open /ads
# (container, CI runner) still get completions. on = report, off = never.
# Reporting an ad nobody saw can get the account reviewed, so set off wherever a
# browser can keep the ad page open.
FREEPI_AUTO_AD=on

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
# FREEPI_BRIDGE_INSECURE=off

# on installs a newer release, off never checks, notify only prints.
FREEPI_AUTO_UPDATE=on

# Mirror for update downloads.
# FREEPI_BRIDGE_DOWNLOAD_BASE=https://github.com/ihsan-ramadhan/freepi-bridge/releases

# Timezone sent with ad requests.
# TZ=UTC
"#;

pub(crate) fn config_path() -> PathBuf {
    state_dir().join("config.env")
}

pub(crate) fn set(key: &str, value: &str) -> Result<(), String> {
    if !KNOWN.contains(&key) {
        return Err(format!("unknown key {key}"));
    }
    if value.contains('\0') || value.contains('\n') {
        return Err(format!("{key} cannot contain a newline or NUL"));
    }
    if parse_value(value) != value {
        return Err(format!(
            "{key} would be truncated on the next read: {value:?} (quote it, or drop the \" #\")"
        ));
    }
    validate(key, value)?;
    ensure_file()?;
    let path = config_path();
    let text = fs::read_to_string(&path)
        .map_err(|err| format!("cannot read {}: {err}", path.display()))?;

    let out = rewrite(&text, key, value);
    let temp = path.with_extension("env.tmp");
    fs::write(&temp, out).map_err(|err| format!("cannot write {}: {err}", temp.display()))?;
    fs::rename(&temp, &path).map_err(|err| {
        let _ = fs::remove_file(&temp);
        format!("cannot replace {}: {err}", path.display())
    })
}

fn rewrite(text: &str, key: &str, value: &str) -> String {
    let mut out = String::with_capacity(text.len() + key.len() + value.len() + 2);
    let mut replaced = false;
    for line in text.lines() {
        let trimmed = line.trim();
        let matches = !trimmed.starts_with('#')
            && trimmed.split_once('=').map(|(k, _)| k.trim()) == Some(key);
        if matches {
            if !replaced {
                out.push_str(&format!("{key}={value}\n"));
                replaced = true;
            }
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    if !replaced {
        out.push_str(&format!("{key}={value}\n"));
    }
    out
}

pub(crate) fn flag(key: &str) -> bool {
    let Some(raw) = crate::state::env_nonempty(key) else {
        return true;
    };
    !matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "0" | "false" | "off" | "no"
    )
}

fn validate(key: &str, value: &str) -> Result<(), String> {
    let value = value.trim();
    let number = |min: u64| -> Result<(), String> {
        let parsed = value
            .parse::<u64>()
            .map_err(|_| format!("{key} needs a whole number, got {value:?}"))?;
        if parsed < min {
            return Err(format!("{key} must be at least {min}, got {parsed}"));
        }
        Ok(())
    };
    match key {
        "PORT" if !value.is_empty() => {
            let port = value
                .parse::<u16>()
                .map_err(|_| format!("PORT needs 1-65535, got {value:?}"))?;
            if port == 0 {
                return Err("PORT needs 1-65535, got 0".to_string());
            }
        }
        "FREEPI_AD_REFRESH_SECS" if !value.is_empty() => number(30)?,
        "FREEPI_MAX_INFLIGHT" | "FREEPI_QUEUE_WAIT_SECS" if !value.is_empty() => number(0)?,
        "FREEPI_BRIDGE_INSECURE" | "FREEPI_AUTO_AD" if !value.is_empty() => {
            let ok = matches!(
                value.to_ascii_lowercase().as_str(),
                "on" | "off" | "true" | "false" | "yes" | "no" | "1" | "0"
            );
            if !ok {
                return Err(format!("{key} needs on or off, got {value:?}"));
            }
        }
        "FREEPI_AUTO_UPDATE" if !value.is_empty() => {
            let ok = matches!(
                value.to_ascii_lowercase().as_str(),
                "on" | "off" | "notify" | "apply" | "check" | "true" | "false" | "yes" | "no"
            );
            if !ok {
                return Err(format!("FREEPI_AUTO_UPDATE needs on, off or notify, got {value:?}"));
            }
        }
        "FREEPI_BASE_URL" | "FREEPI_BRIDGE_DOWNLOAD_BASE"
            if !value.is_empty()
                && !value.starts_with("http://")
                && !value.starts_with("https://") =>
        {
            return Err(format!("{key} needs an http(s) URL, got {value:?}"));
        }
        _ => {}
    }
    Ok(())
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
    fn rewrite_replaces_one_line_and_keeps_comments() {
        let text = TEMPLATE;
        let out = rewrite(text, "FREEPI_AUTO_UPDATE", "off");

        assert_eq!(out.lines().filter(|l| l.starts_with("FREEPI_AUTO_UPDATE=")).count(), 1);
        assert!(out.contains("\nFREEPI_AUTO_UPDATE=off\n"));
        assert!(!out.contains("FREEPI_AUTO_UPDATE=on"));
        assert_eq!(out.lines().count(), text.lines().count());
        assert!(out.contains("# Report ads from the bridge itself"));
        assert!(rewrite("A=1\n", "TZ", "UTC").contains("TZ=UTC\n"));
        assert_eq!(rewrite("PORT=1\nPORT=2\n", "PORT", "3"), "PORT=3\n");
    }

    #[test]
    fn set_rejects_values_the_binary_would_ignore() {
        assert!(set("FREEPI_BRIDGE_DIR", "/tmp").is_err(), "unknown keys are rejected");
        assert!(set("PORT", "abc").is_err(), "non-numeric port is refused");
        assert!(set("PORT", "0").is_err(), "port 0 is refused");
        assert!(set("FREEPI_AD_REFRESH_SECS", "5").is_err(), "refresh under 30 is refused");
        assert!(set("FREEPI_AUTO_UPDATE", "maybe").is_err(), "unknown mode is refused");
        assert!(set("FREEPI_BASE_URL", "api.freepi.ai").is_err(), "URL without scheme is refused");
        assert!(set("TZ", "UTC # comment").is_err(), "a value parse_value would cut is refused");
        assert!(validate("PORT", "").is_ok(), "empty resets a key to its default");
        assert!(validate("FREEPI_AUTO_UPDATE", "").is_ok());
    }

    #[test]
    fn one_boolean_vocabulary_for_every_flag() {
        for key in ["FREEPI_AUTO_AD", "FREEPI_BRIDGE_INSECURE"] {
            for off in ["off", "0", "false", "no", " OFF "] {
                assert!(validate(key, off).is_ok(), "{key}={off} must be accepted");
            }
            for on in ["on", "1", "true", "yes"] {
                assert!(validate(key, on).is_ok(), "{key}={on} must be accepted");
            }
            assert!(validate(key, "maybe").is_err());
        }
    }

    #[test]
    fn shipped_template_is_valid() {
        let parsed = parse(TEMPLATE);
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
        assert!(parsed
            .entries
            .iter()
            .any(|(key, value)| key == "FREEPI_AUTO_AD" && value == "on"));
        assert!(parsed
            .entries
            .iter()
            .any(|(key, value)| key == "FREEPI_AUTO_UPDATE" && value == "on"));
    }

    #[test]
    fn bridge_dir_cannot_be_set_from_the_file() {
        assert!(!KNOWN.contains(&"FREEPI_BRIDGE_DIR"));
    }
}
