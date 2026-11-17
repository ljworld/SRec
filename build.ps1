# SRec - local build script (Windows PowerShell 5+ or PowerShell 7)
#
# Usage (from the project folder):
#   powershell -ExecutionPolicy Bypass -File .\build.ps1            # exe + installer
#   powershell -ExecutionPolicy Bypass -File .\build.ps1 -AppOnly   # just the exe
#
# Output:
#   target\release\srec.exe          the app
#   dist\app\                              portable folder (app + ffmpeg + licences)
#   dist\SRec-Setup-<ver>.exe        installer
#   dist\SHA256SUMS.txt                    checksums

param([switch]$AppOnly)
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

$FFMPEG_URL = "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n8.1-latest-win64-lgpl-8.1.zip"

function Step($t) { Write-Host "`n=== $t ===" -ForegroundColor Cyan }
function Fail($t) { Write-Host "`nERROR: $t" -ForegroundColor Red; exit 1 }

# ---------------------------------------------------------------- 1. tools
Step "Checking tools"
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Fail "Rust not found. Install it, then reopen PowerShell:
    winget install Rustlang.Rustup
    winget install Microsoft.VisualStudio.2022.BuildTools --override `"--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended`""
}
cargo --version

# ---------------------------------------------------------------- 2. build
Step "Building srec.exe (release)"
cargo build --release
if ($LASTEXITCODE -ne 0) { Fail "cargo build failed - copy the errors above and send them for fixing." }
$exe = "target\release\srec.exe"
"{0:N1} MB  {1}" -f ((Get-Item $exe).Length / 1MB), $exe
if ($AppOnly) {
    Write-Host "`nDone. Put an LGPL ffmpeg.exe next to $exe to run it." -ForegroundColor Green
    exit 0
}

$ver = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$app = "dist\app"
Remove-Item dist -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $app | Out-Null

# ---------------------------------------------------------------- 3. ffmpeg
Step "Preparing LGPL FFmpeg"
$cache = "tools\ffmpeg-lgpl"
if (-not (Test-Path "$cache\ffmpeg.exe")) {
    New-Item -ItemType Directory -Force $cache | Out-Null
    Write-Host "Downloading $FFMPEG_URL (one time, ~140 MB)..."
    $ProgressPreference = "SilentlyContinue"
    Invoke-WebRequest $FFMPEG_URL -OutFile "tools\ffmpeg.zip"
    Expand-Archive "tools\ffmpeg.zip" -DestinationPath "tools\ffmpeg-tmp" -Force
    Copy-Item (Get-ChildItem "tools\ffmpeg-tmp" -Recurse -Filter ffmpeg.exe | Select-Object -First 1).FullName "$cache\"
    Copy-Item (Get-ChildItem "tools\ffmpeg-tmp" -Recurse -Filter LICENSE.txt | Select-Object -First 1).FullName "$cache\"
    Remove-Item "tools\ffmpeg.zip", "tools\ffmpeg-tmp" -Recurse -Force
}
Copy-Item "$cache\ffmpeg.exe" "$app\ffmpeg.exe"
Copy-Item "$cache\LICENSE.txt" "$app\FFMPEG-LICENSE.txt"

$verText = & "$app\ffmpeg.exe" -hide_banner -version | Out-String
$conf    = & "$app\ffmpeg.exe" -hide_banner -buildconf | Out-String
if (($verText + $conf) -match "enable-gpl") { Fail "This FFmpeg is a GPL build - use the LGPL variant." }
$enc = & "$app\ffmpeg.exe" -hide_banner -encoders | Out-String
$flt = & "$app\ffmpeg.exe" -hide_banner -filters  | Out-String
foreach ($n in "libsvtav1", "libopus", "av1_nvenc", "av1_qsv", "av1_amf") {
    if ($enc -notmatch $n) { Fail "FFmpeg build lacks $n" }
}
if ($flt -notmatch "ddagrab") { Fail "FFmpeg build lacks ddagrab" }
"FFmpeg bundled with SRec`r`nSource build: $FFMPEG_URL`r`n`r`n$verText`r`n$conf" |
    Out-File -Encoding utf8 "$app\FFMPEG-VERSION.txt"
Write-Host "FFmpeg OK (LGPL, AV1 encoders + Opus + ddagrab)"

# ---------------------------------------------------------------- 3b. whisper
# Offline English subtitles: whisper.cpp (MIT) CPU build + English model +
# Silero voice-activity model. Pinned versions, verified by SHA-256.
Step "Preparing Whisper (subtitles)"
$wcache = "tools\whisper"
$WHISPER = @(
    @{ Name = "whisper-bin-x64.zip"; Url = "https://github.com/ggml-org/whisper.cpp/releases/download/b5454/whisper-bin-x64.zip";
       Sha = "6ba69e3482d7826214f90a6a9c84ca07782aec1e1d0c6a7c30c994fd5d816ccb" },
    @{ Name = "ggml-small.en-q5_1.bin"; Url = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en-q5_1.bin";
       Sha = "bfdff4894dcb76bbf647d56263ea2a96645423f1669176f4844a1bf8e478ad30" },
    @{ Name = "ggml-silero-v6.2.0.bin"; Url = "https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v6.2.0.bin";
       Sha = "2aa269b785eeb53a82983a20501ddf7c1d9c48e33ab63a41391ac6c9f7fb6987" },
    @{ Name = "LICENSE.txt"; Url = "https://raw.githubusercontent.com/ggml-org/whisper.cpp/b5454/LICENSE"; Sha = $null }
)
function Get-Checked($item, $dest) {
    if (-not (Test-Path $dest)) {
        Write-Host "Downloading $($item.Url) (one time)..."
        $ProgressPreference = "SilentlyContinue"
        Invoke-WebRequest $item.Url -OutFile $dest
    }
    if ($item.Sha -and (Get-FileHash $dest -Algorithm SHA256).Hash.ToLower() -ne $item.Sha) {
        Remove-Item $dest -Force
        Fail "Checksum mismatch for $($item.Name) - download removed, run the build again."
    }
}
New-Item -ItemType Directory -Force $wcache | Out-Null
if (-not (Test-Path "$wcache\whisper-cli.exe")) {
    Get-Checked $WHISPER[0] "tools\whisper-bin-x64.zip"
    Expand-Archive "tools\whisper-bin-x64.zip" -DestinationPath "tools\whisper-tmp" -Force
    $rel = (Get-ChildItem "tools\whisper-tmp" -Recurse -Filter whisper-cli.exe | Select-Object -First 1).Directory.FullName
    Copy-Item "$rel\whisper-cli.exe", "$rel\whisper.dll", "$rel\ggml.dll", "$rel\ggml-base.dll" $wcache
    Copy-Item "$rel\ggml-cpu-*.dll" $wcache
    Remove-Item "tools\whisper-tmp" -Recurse -Force
}
foreach ($m in $WHISPER[1..3]) { Get-Checked $m "$wcache\$($m.Name)" }
New-Item -ItemType Directory -Force "$app\whisper" | Out-Null
Copy-Item "$wcache\*" "$app\whisper\"
Copy-Item "$wcache\LICENSE.txt" "$app\WHISPER-LICENSE.txt"
"{0:N0} MB  whisper (subtitles)" -f ((Get-ChildItem "$app\whisper" | Measure-Object Length -Sum).Sum / 1MB)

# ---------------------------------------------------------------- 4. licences
Step "Rust licence list"
$hasAbout = [bool](Get-Command cargo-about -ErrorAction SilentlyContinue)
if ($hasAbout) {
    cargo about generate about.hbs -o "$app\RUST-LICENSES.txt"
    $hasAbout = ($LASTEXITCODE -eq 0)
}
if (-not $hasAbout) {
    Write-Host "(cargo-about not installed or failed - writing crate list. For full texts: cargo install cargo-about)"
    "Rust crates compiled into srec.exe and their licences:" | Out-File -Encoding utf8 "$app\RUST-LICENSES.txt"
    cargo tree -e normal --prefix none --format "{p}  [{l}]" | Sort-Object -Unique |
        Out-File -Append -Encoding utf8 "$app\RUST-LICENSES.txt"
}

# ---------------------------------------------------------------- 5. stage
Step "Staging files"
Copy-Item $exe, LICENSE, TERMS-OF-USE.txt, THIRD-PARTY-NOTICES.txt, docs\USER_MANUAL.md $app
Get-ChildItem $app | Format-Table Name, @{n = "MB"; e = { "{0:N1}" -f ($_.Length / 1MB) } } -AutoSize

# ---------------------------------------------------------------- 6. installer
Step "Building installer"
$iscc = @("${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
          "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
          "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe") | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) {
    Fail "Inno Setup 6 not found. Install it, then run this script again:
    winget install JRSoftware.InnoSetup
(The portable app is already in $app)"
}
$num = ($ver -split "-")[0]
& $iscc "/DAppVersion=$ver" "/DAppNumVersion=$num" "installer\SRec.iss"
if ($LASTEXITCODE -ne 0) { Fail "Inno Setup failed" }

# ---------------------------------------------------------------- 7. checksums
Step "Checksums"
$files = @(Get-ChildItem "dist\SRec-Setup-*.exe") + @(Get-Item "$app\srec.exe", "$app\ffmpeg.exe")
$files | ForEach-Object { "{0}  {1}" -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower(), $_.Name } |
    Out-File -Encoding ascii "dist\SHA256SUMS.txt"
Get-Content "dist\SHA256SUMS.txt"

Write-Host "`nDone:" -ForegroundColor Green
Get-ChildItem "dist\SRec-Setup-*.exe" | ForEach-Object { "  Installer: {0}  ({1:N1} MB)" -f $_.FullName, ($_.Length / 1MB) }
Write-Host "  Portable:  $(Resolve-Path $app)"
