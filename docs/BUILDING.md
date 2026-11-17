# Building from source

## Prerequisites (Windows 10/11, x64)

| Tool | Install |
|---|---|
| Rust (stable, MSVC) | `winget install Rustlang.Rustup`, then `rustup component add rustfmt clippy` |
| Visual Studio C++ Build Tools | `winget install Microsoft.VisualStudio.2022.BuildTools --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"` |
| Inno Setup 6 (installer only) | `winget install JRSoftware.InnoSetup` |
| Git | `winget install Git.Git` |

Reopen PowerShell after installing so `cargo` is on `PATH`.

## One-command build

```powershell
git clone https://github.com/YOUR-GITHUB-USERNAME/SRec.git
cd SRec
powershell -ExecutionPolicy Bypass -File .\build.ps1
```

`build.ps1` does all of this:

1. `cargo build --release` → `target\release\srec.exe`.
2. Downloads the **LGPL FFmpeg 8.1** build (BtbN), about 140 MB, once into `tools\ffmpeg-lgpl`. It checks that the build is not GPL and has `libsvtav1`, `libopus`, `av1_nvenc`, `av1_qsv`, `av1_amf` and `ddagrab`.
3. Downloads **whisper.cpp b5454** (CPU build), **ggml-small.en-q5_1.bin** and **ggml-silero-v6.2.0.bin**, about 200 MB, once into `tools\whisper`. Each is verified against a pinned SHA-256.
4. Writes the Rust licence list (full texts if `cargo install cargo-about` is present).
5. Stages everything in `dist\app\` (the portable folder).
6. Builds `dist\SRec-Setup-<version>.exe` with Inno Setup.
7. Writes `dist\SHA256SUMS.txt`.

Options:

```powershell
.\build.ps1 -AppOnly     # only cargo build; no downloads, no installer
```

Downloads are cached in `tools\` (ignored by git). Delete a file there to download it again.

## Running a dev build

`srec.exe` looks for `ffmpeg.exe` next to itself, then on `PATH`, then in winget's FFmpeg folders. Subtitles need `whisper\whisper-cli.exe` plus the models next to the exe. The easiest way is to run the staged copy:

```powershell
.\build.ps1          # once, to fill dist\app
cargo build --release
copy target\release\srec.exe dist\app\
dist\app\srec.exe
```

## Quality checks (same as CI)

```powershell
cargo fmt --check
cargo clippy --release --all-targets -- -D warnings
cargo test --release
```

The manual process-loopback check captures PC sound for a few seconds and prints the peak level. Play something while it runs:

```powershell
$env:SREC_LOOPBACK_SECS = 4; cargo test --release loopback_captures -- --nocapture
```

## Continuous integration

`.github/workflows/build.yml` runs on every push and pull request:

1. **check:** `fmt`, `clippy -D warnings` and unit tests.
2. **windows:** a full build:
   - FFmpeg (plus its exact source archive, for LGPL compliance)
   - Whisper downloads, cached between runs
   - the licence list
   - the installer, with optional signing
   - checksums
   - artifacts: *SRec-Setup* and *SRec-portable*
3. On `v*` tags it publishes a GitHub Release. Tags with a `-` suffix, such as `v1.0.0-alpha.1`, become **pre-releases**.

### Optional code signing

Add two repository secrets, and the workflow signs both `srec.exe` and the installer:

- `SIGN_PFX_B64`: your `.pfx` certificate, base64-encoded (`[Convert]::ToBase64String([IO.File]::ReadAllBytes("cert.pfx"))`)
- `SIGN_PFX_PASSWORD`: its password

## Project layout

```
SRec/
├─ src/                 Rust sources (see docs/ARCHITECTURE.md)
├─ assets/              App icon (ico + png)
├─ installer/SRec.iss   Inno Setup script
├─ build.ps1            Local build + packaging script
├─ build.rs             Embeds the icon and version info into srec.exe
├─ about.hbs/.toml      cargo-about template / licence allow-list
├─ docs/                User and developer documentation
├─ .github/             CI workflow, issue/PR templates, Dependabot
├─ TERMS-OF-USE.txt     Shown by the installer
└─ THIRD-PARTY-NOTICES.txt
```

Not in git (see `.gitignore`): `target/` (cargo output), `tools/` (downloaded FFmpeg/Whisper), `dist/` (packaged output).
