<div align="center">

# freepi-bridge

**API HTTP yang kompatibel dengan OpenAI untuk akun FreePi.**<br>
Satu binary Rust, tanpa runtime dan tanpa layanan tambahan.

![version](https://img.shields.io/badge/version-0.2.0-blue)
![msrv](https://img.shields.io/badge/msrv-1.89%2B-orange)
![license](https://img.shields.io/badge/license-GPL--3.0--only-blue)

</div>

Bahasa: [English](README.md) | Indonesia

API FreePi mengharuskan kliennya sendiri. Setiap completion butuh header `x-client-version` dan `x-session-id`, dan completion gratis tetap dijeda sampai akun menampilkan satu iklan (`403 ads_required`). Bridge menambahkan header itu, menyajikan halaman iklan, lalu meneruskan completion ke `api.freepi.ai`. Klien apa pun yang kompatibel OpenAI bisa dipakai begitu diarahkan ke bridge.

```
client ──/v1/chat/completions──▶ freepi-bridge ──▶ api.freepi.ai
                                     │
                              /ads (halaman web)
```

## Prasyarat

Bridge tidak punya login sendiri. Dia memakai akun FreePi, jadi butuh JWT akun itu: `FREEPI_TOKEN` di environment, atau file credentials yang ditulis CLI resmi ke `~/.free-pi/agent/credentials.json`. Tanpa salah satu dari keduanya, `freepi-bridge token` tidak mencetak apa pun dan setiap completion gagal dengan `401`.

Cara termudah mendapat file itu: jalankan CLI resmi sekali. Kamu tidak perlu mempertahankannya atau memakainya sebagai editor:

```bash
npx free-pi-cli@latest      # butuh Node.js 22.15+; mengunduh, menjalankan, menyimpan token
```

Itu sekali pakai: masuk dengan GitHub, setujui layar consent, lalu keluar. Setelah itu `~/.free-pi/agent/credentials.json` menyimpan token dan bridge memakainya terus. Menjalankan `npx free-pi-cli@latest` lagi nanti juga cara masuk ulang kalau token kedaluwarsa.

Pasang global kalau tidak mau mengunduh tiap kali, atau kalau kamu juga ingin memakainya sebagai coding agent:

```bash
npm install -g free-pi-cli
free-pi-cli
```

Di mesin yang tidak bisa menjalankan Node atau browser (container, CI runner), lewati CLI dan kirim JWT langsung:

```bash
FREEPI_TOKEN=eyJ... freepi-bridge start --bg
```

## Instalasi

Binary siap pakai tersedia untuk setiap sistem yang didukung, jadi toolchain Rust tidak diperlukan. Pilih platformmu:

| Sistem | Installer | Mendarat di |
|---|---|---|
| Linux x86_64, aarch64 | `install.sh` | `~/.local/bin` |
| macOS 11+ di arm64, x86_64 | `install.sh` | `~/.local/bin` |
| WSL2 | `install.sh` (sama seperti Linux) | `~/.local/bin` |
| Windows 10+ di x86_64 | `install.ps1` | `%LOCALAPPDATA%\Programs\freepi-bridge`, ditambahkan ke PATH pengguna |
| Windows di arm64 | `install.ps1` (build x64, di bawah emulasi) | `%LOCALAPPDATA%\Programs\freepi-bridge` |
| Docker, host apa pun | `docker run`, lihat [Docker](#docker) | hanya di dalam container |
| Selain itu | `cargo install --path .` dari hasil clone | `~/.cargo/bin` |

Linux, macOS dan WSL2:

```bash
curl -fsSL https://raw.githubusercontent.com/ihsan-ramadhan/freepi-bridge/main/install.sh | sh
```

Windows, di PowerShell 5.1 atau lebih baru:

```powershell
irm https://raw.githubusercontent.com/ihsan-ramadhan/freepi-bridge/main/install.ps1 | iex
```

Kedua installer mengambil binary rilis dan memeriksa `.sha256`-nya kalau rilisnya menyediakan.

| Flag atau variabel | Efek |
|---|---|
| `--version v0.1.0` | pasang rilis tertentu, bukan yang terbaru |
| `--from-source` | lewati rilis, bangun dengan `cargo` |
| `--system` | pasang ke `/usr/local/bin` |
| `PREFIX=/opt/x` | pasang ke `PREFIX/bin` |
| `FREEPI_BRIDGE_DOWNLOAD_BASE` | base URL mirror, default `https://github.com/ihsan-ramadhan/freepi-bridge/releases` |

Skrip PowerShell menerima `-Prefix`, `-Version`, `-FromSource` dan `-NoPath`. Pipe ke `iex` tidak menyisakan ruang untuk argumen, jadi nilai yang sama datang dari `FREEPI_BRIDGE_PREFIX`, `FREEPI_BRIDGE_VERSION` dan `FREEPI_BRIDGE_DOWNLOAD_BASE`.

Menjalankan `./install.sh` dari hasil clone akan membangun secara lokal. Kalau tidak ada rilis yang bisa dijangkau, kedua skrip jatuh ke `git clone` plus `cargo build --release`. `cargo install --path .` juga bisa.

### Uninstall

Linux dan macOS: hentikan daemon, hapus binary, hapus direktori state.

```bash
freepi-bridge stop
rm ~/.local/bin/freepi-bridge
rm -rf ~/.freepi-bridge
```

Kalau kamu memasang dengan `PREFIX=/opt/x` atau `--system`, hapus binary dari sana, dan kalau kamu menyetel `FREEPI_BRIDGE_DIR`, hapus direktori itu sebagai ganti `~/.freepi-bridge`.

Windows: installer juga mengubah PATH pengguna, jadi uninstall lewat skrip.

```powershell
irm https://raw.githubusercontent.com/ihsan-ramadhan/freepi-bridge/main/install.ps1 -OutFile install.ps1
.\install.ps1 -Uninstall
```

`-Uninstall` menghentikan daemon, menghapus `%LOCALAPPDATA%\Programs\freepi-bridge`, membuang folder itu dari PATH pengguna, dan menghapus `%USERPROFILE%\.freepi-bridge`. Tambahkan `-KeepState` untuk mempertahankan direktori state beserta log dan config-nya, dan `-Prefix <dir>` kalau kamu memasang di tempat lain. Pemakaian lewat pipe juga jalan: setel `$env:FREEPI_BRIDGE_UNINSTALL='1'` sebelum `irm … | iex`.

Kedua jalur tidak menyentuh `~/.free-pi`; direktori itu milik CLI FreePi resmi.

### Rincian per platform

| Platform | Yang kamu dapat |
|---|---|
| Linux x86_64, aarch64 | binary musl statis; tidak ada versi glibc yang perlu dicocokkan |
| macOS 11+ di arm64 dan x86_64 | binary rilis; installer memilih lewat `uname -m` |
| Windows 10+ di x86_64 | `.exe` rilis; daemon dikelola dengan `tasklist` dan `taskkill` |
| Windows di arm64 | build x64, di bawah emulasi |
| WSL2 | pakai build Linux |
| Docker | runtime `debian-bookworm-slim` |

`install.sh` adalah `sh` POSIX, jadi ia juga jalan di bawah `dash`, `busybox sh` dan `zsh`. Ia membaca checksum dengan `sha256sum` atau `shasum -a 256`, dan melewati verifikasi di mesin yang tidak punya keduanya. `install.ps1` butuh PowerShell 5.1, yang sudah ada di Windows 10. Path state dan log mengikuti `HOME`, atau `USERPROFILE` di Windows.

## Mulai cepat

```bash
freepi-bridge start --bg     # daemon di 127.0.0.1:8787
freepi-bridge doctor         # token, upstream, bridge, bind
```

Arahkan klienmu ke `http://127.0.0.1:8787/v1`:

```bash
curl http://127.0.0.1:8787/v1/chat/completions \
  -H 'content-type: application/json' \
  -d '{"model":"particle","messages":[{"role":"user","content":"hi"}]}'
```

Kalau `BRIDGE_API_KEY` disetel, kirim sebagai bearer token:

```bash
curl http://127.0.0.1:8787/v1/chat/completions \
  -H "authorization: Bearer $BRIDGE_API_KEY" \
  -H 'content-type: application/json' \
  -d '{"model":"particle","messages":[{"role":"user","content":"hi"}]}'
```

Di Windows, `curl` adalah alias untuk `Invoke-WebRequest` di PowerShell 5.1, jadi panggil `curl.exe` atau pakai cmdlet aslinya:

```powershell
$body = '{"model":"particle","messages":[{"role":"user","content":"hi"}]}'
Invoke-RestMethod http://127.0.0.1:8787/v1/chat/completions `
  -Method Post -ContentType 'application/json' -Body $body
```

Bridge memakai ulang JWT akun yang sudah disimpan CLI FreePi resmi di `~/.free-pi/agent/credentials.json`. Kalau `freepi-bridge token` tidak mencetak apa pun, lihat [Prasyarat](#prasyarat).

Bridge melaporkan iklan sendiri, jadi container, VM atau mesin remote tetap dapat completion tanpa browser; `FREEPI_AUTO_AD=0` mematikannya dan meninggalkan `/ads` sebagai satu-satunya cara membersihkan `403 ads_required`. Lihat [Laporan otomatis](#laporan-otomatis).

## Perintah

| Perintah | Fungsinya |
|---|---|
| `freepi-bridge start [--bg] [--port N] [--bind H] [--insecure]` | Jalankan bridge di foreground, atau terlepas dengan `--bg`. Bind non-loopback mewajibkan `BRIDGE_API_KEY` kecuali kamu memberi `--insecure` |
| `freepi-bridge stop` | Hentikan proses terlepas yang tercatat di `state.json`: sinyal sopan dulu, kill paksa setelah 5 s |
| `freepi-bridge restart [--port N] [--bind H]` | Hentikan, lalu mulai terlepas lagi |
| `freepi-bridge status [--json]` | pid, URL, kesehatan dan jumlah model daemon |
| `freepi-bridge models [--json]` | Katalog model langsung dari endpoint `/client-version` FreePi |
| `freepi-bridge doctor [--json]` | Periksa token, upstream, bridge dan keamanan bind |
| `freepi-bridge logs [-f]` | Cetak 200 baris log terakhir; `-f` mengikuti |
| `freepi-bridge token` | Cetak JWT yang dipakai bridge |
| `freepi-bridge ads` | Cetak URL halaman iklan, beserta key kalau ada |
| `freepi-bridge config [--init]` | Cetak file config lokal; `--init` membuatnya kalau belum ada |
| `freepi-bridge update [--check] [--tag TAG]` | Pasang rilis terbaru di tempat; `--check` hanya melapor, `--tag` mengunci satu rilis |
| `freepi-bridge version`, `--version` | Cetak versi |
| `freepi-bridge help`, `--help` | Bantuan global; `<perintah> --help` untuk satu perintah |

Exit code `0` kalau berhasil dan `1` kalau gagal: daemon mati, flag salah, pemeriksaan gagal.

### Environment

Semua variabel di sini juga bisa disimpan di [file config](#konfigurasi); environment tetap menang.

| Variabel | Default | Efek |
|---|---|---|
| `FREEPI_BASE_URL` | `https://api.freepi.ai` | API upstream |
| `FREEPI_TOKEN` | tidak disetel | JWT akun; menimpa file credentials |
| `FREEPI_CREDENTIALS` | `~/.free-pi/agent/credentials.json` | File token yang ditulis CLI resmi |
| `BRIDGE_API_KEY` | tidak disetel | Key bersama yang harus dikirim klien; juga mengunci `/ads` |
| `HOST`, `PORT` | `127.0.0.1`, `8787` | Alamat listen |
| `FREEPI_BRIDGE_DIR` | `~/.freepi-bridge` | Direktori state dan log |
| `FREEPI_BRIDGE_INSECURE` | tidak disetel | `1` berperilaku seperti `--insecure` |
| `FREEPI_AUTO_AD` | `1` | `1` nyala secara default, `0` mati; bridge melaporkan iklan tanpa halaman iklan |
| `FREEPI_AD_REFRESH_SECS` | `600` | Jeda antar laporan iklan otomatis; nilai di bawah 30 diabaikan |
| `FREEPI_AUTO_UPDATE` | tidak disetel | `start --bg` memeriksa sekali sehari: tidak disetel = cetak pemberitahuan, `1` = pasang otomatis, `0` = tidak pernah memeriksa |
| `FREEPI_BRIDGE_DOWNLOAD_BASE` | rilis GitHub | URL mirror untuk unduhan update |
| `TZ` | `UTC` | Dikirim sebagai `x-client-timezone` pada permintaan iklan |

## Konfigurasi

Setelan bisa disimpan di `~/.freepi-bridge/config.env` (atau `$FREEPI_BRIDGE_DIR/config.env`) sebagai ganti shell, jadi daemon yang dijalankan supervisor, cron job, atau setelah reboot tetap memakai setelan yang sama. Kedua installer membuat file ini, dan `start` biasa juga membuatnya, jadi selalu ada yang bisa diedit. File ditulis `0600` di Linux dan macOS (Windows memakai ACL bawaan pengguna) karena bisa berisi `FREEPI_TOKEN` atau `BRIDGE_API_KEY`.

```ini
# freepi-bridge configuration
FREEPI_AUTO_AD=1
FREEPI_AD_REFRESH_SECS=300
BRIDGE_API_KEY=
# PORT=8787
```

Satu `KEY=VALUE` per baris, `#` memulai komentar (beri spasi sebelumnya, atau kutip nilainya, supaya `#` di dalam nilai tetap utuh), dan tanda kutip berpasangan di sekitar nilai dihapus. Key yang tidak dikenal dilaporkan di stderr dan diabaikan, jadi salah ketik tidak diam-diam mengubah cara bridge berjalan. Environment asli menang atas file, sehingga override sekali pakai tetap jalan (`FREEPI_AUTO_AD=0 freepi-bridge start --bg`). Perubahan berlaku saat start berikutnya: `freepi-bridge restart`.

```bash
freepi-bridge config            # cetak path dan isi file saat ini
freepi-bridge config --init     # buat kalau belum ada; tidak pernah menimpa
```

`FREEPI_BRIDGE_DIR` satu-satunya variabel yang tidak bisa disetel dari file, karena dialah yang menentukan lokasi file dibaca.

## Update

`freepi-bridge update` menukar binary yang sedang dipakai dengan rilis terbaru: ia membaca `releases/latest` di GitHub, mengunduh aset untuk platform ini, mencocokkan `.sha256` yang diterbitkan, mengganti binary, lalu menyalakan ulang daemon kalau sebelumnya berjalan.

```bash
freepi-bridge update                # pasang rilis terbaru
freepi-bridge update --check        # hanya melapor apakah ada versi baru
freepi-bridge update --tag v0.1.2   # pasang rilis tertentu, atau turun versi
```

Perintah yang sama jalan di Linux x86_64/aarch64, macOS arm64/x86_64 dan Windows 10+ x86_64. Ia hanya menulis di sebelah binary, jadi pemasangan yang butuh root (`/usr/local/bin`) harus diperbarui dengan `sudo`. Unduhan gagal atau checksum tidak cocok tidak menyentuh binary yang sedang dipakai. Di Windows, image yang berjalan diganti nama jadi `freepi-bridge.exe.old` dan dihapus saat start berikutnya.

`start --bg`, dan otomatis juga `restart`, memeriksa paling banyak sekali sehari (stempelnya disimpan di direktori state) lalu mencetak satu baris kalau ada rilis lebih baru. `FREEPI_AUTO_UPDATE=1` memasangnya sebelum daemon jalan; `FREEPI_AUTO_UPDATE=0` tidak pernah memeriksa. `start` di foreground, yang dipakai Docker dan process supervisor, tidak pernah memperbarui diri: bangun image baru atau jalankan ulang installer di sana.

## Endpoint

| Method | Path | Kegunaan |
|---|---|---|
| `GET` | `/v1/models` | Daftar model dari `/client-version` FreePi, di-cache 5 menit |
| `POST` | `/v1/chat/completions` | Chat completion, termasuk passthrough SSE streaming |
| `GET` | `/ads/next?slot=inline\|banner` | Creative iklan untuk akun ini |
| `POST` | `/ads/impression` | Laporkan `{ad_id, click_token, imp_fired}` |
| `GET` | `/ads` | Halaman yang merender iklan dan melaporkan impression |
| `GET` | `/health`, `/healthz` | Probe liveness |

Bridge mencari token dengan urutan ini:

1. JWT dari klien di `Authorization: Bearer eyJ...`, hanya diterima kalau `BRIDGE_API_KEY` tidak disetel
2. `FREEPI_TOKEN`
3. file credentials (`FREEPI_CREDENTIALS`, kalau tidak ada `~/.free-pi/agent/credentials.json`)

## Iklan

FreePi dibiayai iklan. Akun tanpa impression terbaru mendapat `403 ads_required` dan completion tetap dijeda sampai halaman iklan berjalan. Bridge mengikuti protokol yang sama dengan CLI resmi:

- `GET /ads/next?slot=inline` mengembalikan `{ad_id, click_token, creative:{headline,body,cta}, click_url, imp_url, source}`
- `POST /ads/impression {ad_id, click_token, imp_fired}` melaporkan tampilan
- `GET /ads?key=<BRIDGE_API_KEY>` merender creative, menunggu sampai terlihat (`IntersectionObserver` plus visibilitas halaman), menembakkan pixel jaringan iklan, lalu melaporkan impression

```bash
freepi-bridge ads     # mencetak URL untuk dibuka
```

Biarkan halaman tetap terbuka selama kamu bekerja. Melaporkan impression untuk creative yang tidak pernah dilihat siapa pun adalah penipuan impression dan bisa membuat akun ditinjau. Kredit yang dibeli di akun menghapus kewajiban ini.

### Host tanpa layar

`/ads` hanya mendengarkan di port bridge sendiri. Container atau CI runner tidak bisa membukanya, jadi bridge melaporkan iklan sendiri dan mengulang satu completion yang terhalang sekali; itulah kerja `FREEPI_AUTO_AD`, dan itu sebabnya ia nyala sejak awal. `FREEPI_AUTO_AD=0` membiarkan akun apa adanya, dan host seperti itu akan terus menjawab `403 ads_required`.

### Laporan otomatis

Bridge melaporkan iklan sendiri setiap `FREEPI_AD_REFRESH_SECS` detik, jadi completion tetap jalan di container, CI runner, atau host mana pun yang tidak bisa membuka `/ads`. `FREEPI_AUTO_AD` dibaca sekali, saat proses mulai, dari environment atau [file config](#konfigurasi); `0` mematikan laporan otomatis.

```bash
freepi-bridge token          # kosong? lihat Prasyarat
freepi-bridge start --bg
freepi-bridge logs | grep 'ad impression'
# ad impression reported (session ..., gravity false)
```

`start --bg` menyalin environment-nya ke child yang terlepas, jadi setelan itu ikut. `restart` membaca ulang shell dan file config perintah yang menjalankannya, bukan daemon yang digantikannya, sehingga `FREEPI_AUTO_AD=0 freepi-bridge restart` memakai tampilan iklan asli untuk satu kali jalan; tulis `FREEPI_AUTO_AD=0` di `~/.freepi-bridge/config.env` supaya permanen. Supervisor bekerja dengan cara yang sama: `Environment=FREEPI_AUTO_AD=0` di unit systemd, `-e FREEPI_AUTO_AD=0` untuk `docker run`.

`freepi-bridge ads` mencetak halaman yang bisa dibuka kalau kamu lebih suka melihat iklannya sungguhan.

Laporan otomatis mengklaim iklan yang tidak pernah dilihat siapa pun, itu penipuan impression dan bisa membuat akun ditinjau; setel `FREEPI_AUTO_AD=0` di mesin mana pun yang browsernya bisa membiarkan `/ads` terbuka.

## Docker

```bash
docker build -t freepi-bridge .
docker run -d --name freepi-bridge -p 8787:7860 \
  -e FREEPI_TOKEN=... -e BRIDGE_API_KEY=... freepi-bridge
```

## Docker Compose

```bash
cp .env.example .env      # isi FREEPI_TOKEN dan BRIDGE_API_KEY
docker compose up -d --build
docker compose logs -f
```

Container berjalan sebagai pengguna non-root, bind ke `0.0.0.0:7860`, dan membawa healthcheck `/health`. Karena bind ke `0.0.0.0`, bridge mewajibkan `BRIDGE_API_KEY` (atau `FREEPI_BRIDGE_INSECURE=1`), jadi run tanpa salah satunya langsung berhenti. State dan `config.env` berada di dalam container; mount `/home/bridge/.freepi-bridge` kalau mau keduanya bertahan setelah rebuild. Token datang lewat `FREEPI_TOKEN`.

## Pengembangan

```bash
cargo test                       # upstream mock: auth, injeksi header, SSE, iklan
cargo build --release
PREFIX=/tmp/out ./install.sh     # build lokal, tanpa unduh rilis
```

MSRV 1.89, ditentukan oleh `uuid` 1.29. CI menjalankan test suite di stable dan di 1.89, jadi badge tidak bisa melenceng tanpa build merah.

Diukur pada i5-13420H terhadap upstream mock lokal dengan 50 koneksi bersamaan: sekitar 30k req/s, tambahan 1.5 ms p50 per request, RSS 6.8 MB, binary 2.8 MB. Completion FreePi sungguhan butuh beberapa detik, jadi hop proxy tidak terasa di praktik.

## Lisensi

GPL-3.0-only. Lihat [LICENSE](LICENSE).
