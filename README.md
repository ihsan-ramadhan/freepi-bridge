<div align="center">

# freepi-bridge

**An OpenAI-compatible HTTP API for a FreePi account.**<br>
A single Rust binary, no runtime and no extra services.

![version](https://img.shields.io/badge/version-0.1.1-blue)
![msrv](https://img.shields.io/badge/msrv-1.89%2B-orange)
![license](https://img.shields.io/badge/license-GPL--3.0--only-blue)

</div>

FreePi's API expects its own client. Every completion needs `x-client-version` and `x-session-id` headers, and free completions stay paused until the account has shown an ad (`403 ads_required`). The bridge adds those headers, serves the ad page, and forwards completions to `api.freepi.ai`. Any OpenAI-compatible client works once it points at the bridge.

```
client ──/v1/chat/completions──▶ freepi-bridge ──▶ api.freepi.ai
                                     │
                              /ads (web page)
```

## Install

Linux and macOS:

```bash
curl -fsSL https://raw.githubusercontent.com/ihsan-ramadhan/freepi-bridge/main/install.sh | sh
```

Windows (PowerShell 5.1 or newer):

```powershell
irm https://raw.githubusercontent.com/ihsan-ramadhan/freepi-bridge/main/install.ps1 | iex
```

Both installers fetch the release binary, check its `.sha256` when the release publishes one, and put it on your PATH. Linux and macOS land in `~/.local/bin`. Windows lands in `%LOCALAPPDATA%\Programs\freepi-bridge` and adds that folder to the user PATH. A Rust toolchain is not needed.

| Flag or variable | Effect |
|---|---|
| `--version v0.1.0` | install a pinned release instead of the latest |
| `--from-source` | skip the release and build with `cargo` |
| `--system` | install into `/usr/local/bin` |
| `PREFIX=/opt/x` | install into `PREFIX/bin` |
| `FREEPI_BRIDGE_DOWNLOAD_BASE` | mirror base URL, default `https://github.com/ihsan-ramadhan/freepi-bridge/releases` |

The PowerShell script takes `-Prefix`, `-Version`, `-FromSource` and `-NoPath`. Piping into `iex` leaves no room for arguments, so the same values come from `FREEPI_BRIDGE_PREFIX`, `FREEPI_BRIDGE_VERSION` and `FREEPI_BRIDGE_DOWNLOAD_BASE`.

Running `./install.sh` from a clone builds locally instead. If no release is reachable, both scripts fall back to `git clone` plus `cargo build --release`. `cargo install --path .` also works.

### Uninstall

Linux and macOS: stop the daemon, delete the binary, delete the state directory.

```bash
freepi-bridge stop
rm ~/.local/bin/freepi-bridge
rm -rf ~/.freepi-bridge
```

If you installed with `PREFIX=/opt/x` or `--system`, delete the binary from there instead, and if you set `FREEPI_BRIDGE_DIR`, delete that directory instead of `~/.freepi-bridge`.

Windows: the installer also edits your user PATH, so uninstall through the script.

```powershell
irm https://raw.githubusercontent.com/ihsan-ramadhan/freepi-bridge/main/install.ps1 -OutFile install.ps1
.\install.ps1 -Uninstall
```

`-Uninstall` stops the daemon, deletes `%LOCALAPPDATA%\Programs\freepi-bridge`, removes that folder from the user PATH, and deletes `%USERPROFILE%\.freepi-bridge`. Add `-KeepState` to keep the state directory and its log, and `-Prefix <dir>` if you installed somewhere else. Piped usage works too: set `$env:FREEPI_BRIDGE_UNINSTALL='1'` before `irm … | iex`.

Both paths leave `~/.free-pi` alone; that directory belongs to the official FreePi CLI.

### Platforms

| Platform | What you get |
|---|---|
| Linux x86_64, aarch64 | static musl binary; no glibc version to match |
| macOS 11+ on arm64 and x86_64 | release binary; the installer picks by `uname -m` |
| Windows 10+ on x86_64 | release `.exe`; the daemon is managed with `tasklist` and `taskkill` |
| Windows on arm64 | the x64 build, under emulation |
| WSL2 | use the Linux build |
| Docker | `debian-bookworm-slim` runtime |

`install.sh` is POSIX `sh`. It reads checksums with `sha256sum` or `shasum -a 256`, and skips verification on machines that have neither. State and log paths follow `HOME`, or `USERPROFILE` on Windows.

## Quick start

```bash
freepi-bridge start --bg     # daemon on 127.0.0.1:8787
freepi-bridge doctor         # token, upstream, bridge, bind
```

Point your client at `http://127.0.0.1:8787/v1`:

```bash
curl http://127.0.0.1:8787/v1/chat/completions \
  -H 'content-type: application/json' \
  -d '{"model":"particle","messages":[{"role":"user","content":"hi"}]}'
```

With `BRIDGE_API_KEY` set, send it as the bearer token:

```bash
curl http://127.0.0.1:8787/v1/chat/completions \
  -H "authorization: Bearer $BRIDGE_API_KEY" \
  -H 'content-type: application/json' \
  -d '{"model":"particle","messages":[{"role":"user","content":"hi"}]}'
```

The bridge reuses the account JWT that the official `free-pi` CLI already stored. Run `free-pi` once in a terminal if `freepi-bridge token` prints nothing.

## Commands

| Command | What it does |
|---|---|
| `freepi-bridge start [--bg] [--port N] [--bind H] [--insecure]` | Run the bridge in the foreground, or detached with `--bg`. A non-loopback bind requires `BRIDGE_API_KEY` unless you pass `--insecure` |
| `freepi-bridge stop` | Stop the detached process recorded in `state.json`: graceful signal first, forced kill after 5 s |
| `freepi-bridge restart [--port N] [--bind H]` | Stop, then start detached again |
| `freepi-bridge status [--json]` | pid, URL, health and model count of the daemon |
| `freepi-bridge models [--json]` | Live model catalog from the FreePi `/client-version` endpoint |
| `freepi-bridge doctor [--json]` | Check token, upstream, bridge and bind safety |
| `freepi-bridge logs [-f]` | Print the last 200 log lines; `-f` follows |
| `freepi-bridge token` | Print the JWT the bridge uses |
| `freepi-bridge ads` | Print the ad page URL, with the key when one is set |
| `freepi-bridge version`, `--version` | Print the version |
| `freepi-bridge help`, `--help` | Global help; `<command> --help` for one command |

Exit code is `0` on success and `1` on error: daemon down, bad flag, failed check.

### Environment

| Variable | Default | Effect |
|---|---|---|
| `FREEPI_BASE_URL` | `https://api.freepi.ai` | Upstream API |
| `FREEPI_TOKEN` | unset | Account JWT; overrides the credentials file |
| `FREEPI_CREDENTIALS` | `~/.free-pi/agent/credentials.json` | Token file written by the official CLI |
| `BRIDGE_API_KEY` | unset | Shared key clients must send; also gates `/ads` |
| `HOST`, `PORT` | `127.0.0.1`, `8787` (`0.0.0.0`, `7860` on a Space) | Listen address |
| `FREEPI_BRIDGE_DIR` | `~/.freepi-bridge` | State and log directory |
| `FREEPI_BRIDGE_INSECURE` | unset | `1` behaves like `--insecure` |
| `FREEPI_AUTO_AD` | `0` | `1` on, `0` off; the bridge reports an ad without the ad page |
| `FREEPI_AD_REFRESH_SECS` | `600` | Seconds between automatic ad reports; values under 30 are ignored |
| `TZ` | system timezone | Sent as `x-client-timezone` on ad requests |

## Endpoints

| Method | Path | Purpose |
|---|---|---|
| `GET` | `/v1/models` | Model list from FreePi `/client-version`, cached for 5 minutes |
| `POST` | `/v1/chat/completions` | Chat completions, including streaming SSE passthrough |
| `GET` | `/ads/next?slot=inline\|banner` | Ad creative for this account |
| `POST` | `/ads/impression` | Report `{ad_id, click_token, imp_fired}` |
| `GET` | `/ads` | Page that renders an ad and reports the impression |
| `GET` | `/health`, `/healthz` | Liveness probe |

The bridge looks for a token in this order:

1. `FREEPI_TOKEN`
2. the credentials file (`FREEPI_CREDENTIALS`, else `~/.free-pi/agent/credentials.json`)
3. a JWT from the client in `Authorization: Bearer eyJ...`, accepted only when `BRIDGE_API_KEY` is unset

## Ads

FreePi is ad-funded. An account with no recent impression gets `403 ads_required` and completions stay paused until the ad page runs. The bridge follows the same protocol as the official CLI:

- `GET /ads/next?slot=inline` returns `{ad_id, click_token, creative:{headline,body,cta}, click_url, imp_url, source}`
- `POST /ads/impression {ad_id, click_token, imp_fired}` reports the view
- `GET /ads?key=<BRIDGE_API_KEY>` renders the creative, waits until it is visible (`IntersectionObserver` plus page visibility), fires the ad network pixel, then reports the impression

```bash
freepi-bridge ads     # prints the URL to open
```

Keep the page open while you work. Reporting an impression for a creative nobody saw is impression fraud and can get the account reviewed. Credits bought on the account remove the requirement.

### Headless hosts

`/ads` only listens on the bridge's own port. A container, a Space or a CI runner cannot open it, so the bridge never reports an ad and FreePi answers `403 ads_required`. `FREEPI_AUTO_AD=1` makes the bridge report an ad by itself and retry a gated completion once. `0` or unset leaves the account alone.

The flag reports ads the bridge never displayed. Use it only where the ad page is unreachable.

## Docker

```bash
docker build -t freepi-bridge .
docker run -d --name freepi-bridge -p 8787:7860 \
  -e FREEPI_TOKEN=... -e BRIDGE_API_KEY=... freepi-bridge
```

## Docker Compose

```bash
cp .env.example .env      # set FREEPI_TOKEN and BRIDGE_API_KEY
docker compose up -d --build
docker compose logs -f
```

The container runs as a non-root user, binds `0.0.0.0:7860`, and ships a `/health` healthcheck. State stays outside the container; the token arrives through `FREEPI_TOKEN`.

## Development

```bash
cargo test                       # mock upstream: auth, header injection, SSE, ads
cargo build --release
PREFIX=/tmp/out ./install.sh     # local build, no release download
```

The MSRV is 1.89, set by `uuid` 1.29. CI runs the suite on stable and on 1.89, so the badge cannot drift without a red build.

Tagging cuts a release. `git tag v0.1.0 && git push --tags` runs `release.yml`, which builds static `linux-x86_64` and `linux-aarch64` tarballs, `macos-x86_64` and `macos-aarch64` tarballs, and a `windows-x86_64` zip, each with a `.sha256` beside it.

Measured on an i5-13420H against a local mock upstream at 50 concurrent connections: about 30k req/s, 1.5 ms p50 added per request, 6.8 MB RSS, 2.8 MB binary. Real FreePi completions take seconds, so the proxy hop does not show up in practice.

## License

GPL-3.0-only. See `LICENSE`.
