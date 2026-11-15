<div align="center">

# freepi-bridge

**HTTP API kompatibel OpenAI untuk akun FreePi.**<br>
Satu binary Rust, tanpa runtime dan tanpa layanan tambahan.

![version](https://img.shields.io/badge/version-0.2.1-blue)
![msrv](https://img.shields.io/badge/msrv-1.89%2B-orange)
![license](https://img.shields.io/badge/license-GPL--3.0--only-blue)

</div>

Language: [English](README.md) | Indonesia

API kompatibel OpenAI untuk akun FreePi. Satu binary Rust statis, tanpa runtime.

API FreePi hanya menerima klien resminya sendiri dan butuh impresi iklan sebelum
completion gratis bisa dipakai. Bridge ini menangani keduanya dan meneruskan
request ke `api.freepi.ai`, sehingga aplikasi apa pun yang kompatibel OpenAI
(opencode, pi, omp, 9router, dll) bisa memakai akun FreePi kamu.

FreePi itu akun terpisah. Pendaftarannya lewat login GitHub, dan bridge memakai
kuota akun itu, bukan membuatkannya, jadi [Token](#token) harus dilakukan sekali
di suatu mesin.

## Mulai cepat

1. **Instal** (lihat [Instalasi](#instalasi))
2. **Login ke FreePi sekali**: `npx free-pi-cli@latest` (butuh Node.js 22.15+).
   Login dengan GitHub lalu keluar. Token tersimpan dan dipakai ulang. Tidak ada
   Node.js di mesin ini? Pakai [Docker](#docker), atau jalankan CLI sekali di
   mesin lain lalu salin `~/.free-pi/agent/credentials.json` ke sini.
3. **Jalankan bridge**:
   ```sh
   freepi-bridge start --bg
   freepi-bridge doctor     # semua harus berstatus OK
   ```
4. **Arahkan aplikasimu ke bridge**:
   - Base URL: `http://127.0.0.1:8787/v1`
   - API key: `BRIDGE_API_KEY` jika kamu mengaturnya, kalau tidak isi placeholder apa saja
   - Model: pilih dari hasil `freepi-bridge models`

Coba:

```sh
curl http://127.0.0.1:8787/v1/chat/completions \
  -H 'content-type: application/json' \
  -d '{"model":"particle","messages":[{"role":"user","content":"hi"}]}'
```

`freepi-bridge models` mencetak seluruh katalog. `deepseek-v4.1-flash` pilihan
bagus untuk coding.

> **Windows PowerShell 5.1:** `curl` adalah alias untuk `Invoke-WebRequest`.
> Pakai `curl.exe`, atau:
> ```powershell
> Invoke-RestMethod http://127.0.0.1:8787/v1/chat/completions -Method Post `
>   -ContentType 'application/json' `
>   -Body '{"model":"particle","messages":[{"role":"user","content":"hi"}]}'
> ```

## Instalasi

Linux, macOS, atau WSL2:

```sh
curl -fsSL https://raw.githubusercontent.com/ihsan-ramadhan/freepi-bridge/main/install.sh | sh
```

Windows 10 atau lebih baru, di PowerShell 5.1 atau lebih baru:

```powershell
irm https://raw.githubusercontent.com/ihsan-ramadhan/freepi-bridge/main/install.ps1 | iex
```

Dari source, dengan toolchain Rust:

```sh
cargo install --path .
```

Container: lihat [Docker](#docker).

| Dipasang dengan | Mendarat di |
| --- | --- |
| `install.sh` | `~/.local/bin` |
| `install.ps1` | `%LOCALAPPDATA%\Programs\freepi-bridge`, ditambahkan ke PATH |
| `cargo install --path .` | `~/.cargo/bin` |

Didukung: Linux x86_64/aarch64, macOS 11+ (Intel dan Apple Silicon), Windows
x86_64 (arm64 menjalankan build x64 lewat emulasi). Installer tidak butuh
toolchain Rust. Restart terminal setelah instalasi agar PATH baru terbaca.

Opsi installer: `--version v0.2.1` (pin rilis), `--system` (instal ke
`/usr/local/bin`), `PREFIX=/opt/x`, `--from-source`. PowerShell: `-Version`,
`-Prefix`, `-FromSource`, `-NoPath`. Pipe ke `iex` tidak menyisakan ruang untuk
argumen, jadi setel `$env:FREEPI_BRIDGE_VERSION`, `$env:FREEPI_BRIDGE_PREFIX`
dan kawan-kawannya.

## Token

Bridge memakai akun FreePi kamu, jadi butuh JWT-nya. Urutan pencariannya:

1. Environment variable `FREEPI_TOKEN`
2. `~/.free-pi/agent/credentials.json` (dibuat oleh `npx free-pi-cli@latest`)
3. JWT `Bearer` yang dikirim klien (hanya jika `BRIDGE_API_KEY` tidak diatur)

Di mesin tanpa Node atau browser (container, CI, server), login di mesin lain
lalu berikan token-nya: `FREEPI_TOKEN=eyJ... freepi-bridge start --bg`.
Jika token kedaluwarsa, jalankan `npx free-pi-cli@latest` lagi.

## Iklan

Completion gratis mengembalikan `403 ads_required` sampai akun melaporkan
impresi iklan. Secara default bridge melaporkannya otomatis setiap 10 menit
(`FREEPI_AUTO_AD=1`), jadi tetap jalan di host tanpa browser. Ini melaporkan
iklan yang tidak dilihat siapa pun dan bisa membuat akun ditinjau.

Jika bisa membiarkan browser tetap terbuka, atur `FREEPI_AUTO_AD=0` dan buka
halaman yang ditampilkan `freepi-bridge ads`. Membeli kredit di akun juga
menghapus syarat ini.

Mendapat `403 ads_required` padahal browser terbuka biasanya berarti halamannya
tidak dihitung: tab tersembunyi dan jendela yang diminimalkan tidak pernah
melaporkan impression. Biarkan `/ads` terlihat, lalu pastikan laporannya masuk:

```sh
freepi-bridge logs | grep 'ad impression'
```

## Perintah

| Perintah | Fungsi |
| --- | --- |
| `start [--bg] [--port N] [--bind H] [--insecure]` | jalankan di foreground, atau di background dengan `--bg` |
| `stop`, `restart` | kontrol daemon |
| `status`, `logs [-f]` | pantau daemon |
| `models` | tampilkan model yang tersedia |
| `doctor` | cek token, upstream, bridge, bind |
| `token`, `ads` | tampilkan JWT yang dipakai / URL halaman iklan |
| `config [--init]` | tampilkan atau buat `config.env` |
| `update [--check] [--tag TAG]` | instal rilis terbaru (atau rilis tertentu) |

Exit code `0` jika sukses, `1` jika error. Tambahkan `--json` pada `status`,
`models`, dan `doctor` untuk keperluan script.

## Konfigurasi

Atur variabel lewat environment atau `~/.freepi-bridge/config.env`
(`KEY=VALUE`, satu per baris). Environment lebih diutamakan. Jalankan
`freepi-bridge restart` setelah mengedit.

| Variabel | Default | Fungsi |
| --- | --- | --- |
| `FREEPI_TOKEN` | tidak diatur | JWT akun |
| `BRIDGE_API_KEY` | tidak diatur | key yang wajib dikirim klien; juga melindungi `/ads` |
| `HOST`, `PORT` | `127.0.0.1`, `8787` | alamat listen |
| `FREEPI_AUTO_AD` | `1` | lapor iklan otomatis, `0` untuk mematikan |
| `FREEPI_AD_REFRESH_SECS` | `600` | jeda antar laporan dalam detik (min 30) |
| `FREEPI_AUTO_UPDATE` | tidak diatur | `start --bg` dan `restart` memeriksa paling banyak sekali sehari: tidak diatur = cetak pemberitahuan, `1` = pasang rilisnya, `0` = tidak pernah memeriksa. Daemon yang sedang berjalan tidak pernah memeriksa sendiri |
| `FREEPI_BRIDGE_DIR` | `~/.freepi-bridge` | direktori state dan log |

Bind ke alamat non-loopback (`--bind 0.0.0.0`) mewajibkan `BRIDGE_API_KEY`
kecuali memakai `--insecure`.

## Docker

```sh
docker build -t freepi-bridge .
docker run -d --name freepi-bridge -p 8787:7860 \
  -e FREEPI_TOKEN=... -e BRIDGE_API_KEY=... freepi-bridge
```

Atau: `cp .env.example .env`, isi kedua nilai, lalu
`docker compose up -d --build`. Container berjalan sebagai non-root di port
7860 dan punya health check `/health`. Mount `/home/bridge/.freepi-bridge`
agar state tetap ada setelah rebuild.

`-e BRIDGE_API_KEY=...` bukan opsional: image bind ke `0.0.0.0:7860`, dan bridge
berhenti pada bind non-loopback tanpa key. Pemetaan `8787:7860` dipakai karena
8787 adalah port default bridge di luar container.

## Update dan uninstall

```sh
freepi-bridge update         # rilis terbaru; --check hanya untuk mengecek
```

| | Linux / macOS | Windows |
| --- | --- | --- |
| Uninstall | `freepi-bridge stop`<br>`rm ~/.local/bin/freepi-bridge`<br>`rm -rf ~/.freepi-bridge` | `.\install.ps1 -Uninstall` (tambah `-KeepState` untuk menyimpan log/config) |

Diinstal dengan `--system` atau `PREFIX`? Hapus binary dari lokasi tersebut.
`~/.free-pi` milik FreePi CLI resmi dan tidak pernah disentuh.

## Pemecahan masalah

| Masalah | Solusi |
| --- | --- |
| `freepi-bridge: command not found` | restart terminal; pastikan `~/.local/bin` ada di PATH |
| `401` | token tidak ditemukan: jalankan `freepi-bridge token`, jika kosong lihat [Token](#token) |
| `403 ads_required` | lihat [Iklan](#iklan); `FREEPI_AUTO_AD=1` membersihkannya tanpa browser |
| `403` padahal browser terbuka | halamannya tidak dihitung; biarkan `/ads` terlihat dan cek `freepi-bridge logs \| grep 'ad impression'` |
| `409 concurrent_session` | akun sudah memegang empat slot sesi CLI; bridge memanggil `/session/reset` lalu mengulang sekali, dan kalau tetap gagal berarti mesin lain memegang slot |
| `409 model_at_capacity` | model itu sedang penuh di upstream; tunggu beberapa menit atau pilih model gratis lain |
| `429 concurrent` | satu request aktif per akun; klien lain atau agent kedua memakai akun FreePi yang sama |
| `429 hourly_ceiling` | batas per jam tercapai, tunggu reset. Iklan tidak bisa dilewati, dan batasnya per akun |
| `429 premium_allowance` | kuota premium habis; pindah ke model bertanda `Free Unlimited` di `freepi-bridge models` (`particle`, `deepseek-v4.1-flash`, `glm-5.3-flash`, `deepseek-v4-flash`), karena akhiran `-flash` tidak membuat model jadi gratis |
| lainnya | jalankan `freepi-bridge doctor`, lalu `freepi-bridge logs` |

## Endpoint

`GET /v1/models` · `POST /v1/chat/completions` (streaming SSE) · `GET /ads` ·
`GET /health`

## Pengembangan

```sh
cargo test
cargo build --release
```

MSRV Rust 1.89. CI berjalan di stable dan 1.89.

## Lisensi

GPL-3.0-only. Lihat [LICENSE](LICENSE).