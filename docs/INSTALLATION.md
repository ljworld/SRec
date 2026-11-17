# Installation guide

## 1. Check the requirements

| | Minimum | Recommended |
|---|---|---|
| Windows | Windows 10 version 2004 (build 19041), 64-bit | Windows 11 |
| Graphics | Any DirectX 11 GPU | NVIDIA RTX 40+, AMD RX 7000+, Intel Arc / Core Ultra (hardware AV1) |
| CPU | 4 cores | 6+ cores (needed for 1080p30 when there's no AV1 GPU) |
| Disk | 300 MB for the app | Plus space for recordings: about 0.1–1.5 GB per hour |

To find your Windows version, press `Win+R`, type `winver`, and press Enter.

## 2. Download

1. Open the project's **[Releases](https://github.com/YOUR-GITHUB-USERNAME/SRec/releases)** page.
2. Download **`SRec-Setup-<version>.exe`** (about 230 MB, because it includes FFmpeg and the speech model).
3. Optional, but recommended: check that the download isn't corrupted or tampered with. Download `SHA256SUMS.txt` from the same release and run this in PowerShell in your Downloads folder:

   ```powershell
   Get-FileHash .\SRec-Setup-*.exe -Algorithm SHA256
   ```

   The hash must match the line for the setup file in `SHA256SUMS.txt`.

## 3. Install

1. Double-click `SRec-Setup-<version>.exe`.
2. **If Windows shows "Windows protected your PC"** (SmartScreen): until the project has a code-signing certificate, Windows warns about new, unsigned apps. Click **More info → Run anyway**.
3. Choose **Install for me only** (recommended, no admin rights needed) or for all users.
4. Read the **Terms of Use**, select *I accept*, and continue.
5. Choose the optional tasks:
   - *Create a desktop shortcut*
   - *Start SRec when I sign in to Windows*
6. Click **Install**, then **Finish**. SRec starts if *Launch SRec now* is ticked.

The installer works fully offline. Everything SRec needs is inside it.

### Where things go

| What | Where |
|---|---|
| Program (per-user install) | `%LOCALAPPDATA%\Programs\SRec` |
| Program (all-users install) | `C:\Program Files\SRec` |
| Speech model and whisper.cpp | `<program folder>\whisper\` |
| Settings | `%APPDATA%\SRec\config.ini` |
| Recordings and subtitles | `%USERPROFILE%\Videos\SRec Recordings` |
| Logs (only kept when something fails) | `Videos\SRec Recordings\logs` |

## 4. First start

When SRec starts, the island appears at the top centre of the screen and shows *Checking graphics card...*. SRec test-encodes a few frames with each AV1 encoder your PC might support, and keeps the fastest one that works. This takes a few seconds once; later starts use the cached result.

When the island shows something like `Quick Sync AV1 · Full screen · 700 MB/h`, it's ready. See the [User manual](USER_MANUAL.md) next.

### Microphone permission

If your voice is missing from recordings, allow desktop apps to use the microphone: **Settings → Privacy & security → Microphone → Let desktop apps access your microphone → On**. SRec warns on the island if the microphone delivers only silence.

## 5. Update

Download the newer setup file and run it. It replaces the old version and keeps your settings and recordings. You don't need to uninstall first. Close SRec before updating; the installer closes it if it's still running.

Upgrading from **AV1 Island** (SRec's old name, v0.3.x and earlier) works the same way. The old program, shortcuts and autostart entry are removed, and your settings are carried over. If you used *start when I sign in*, tick it again during setup.

## 6. Uninstall

Use **Settings → Apps → Installed apps → SRec → Uninstall**, or **Start menu → SRec → Uninstall SRec**.

This removes the program, its settings and the autostart entry. **Your recordings and subtitles in `Videos\SRec Recordings` are kept.** Delete that folder yourself if you don't need them.

## 7. Portable use (no installer)

Each release build also produces a portable folder (`dist\app` when you [build from source](BUILDING.md), or the *SRec-portable* artifact of a CI run). Copy the whole folder anywhere and run `srec.exe`. Keep `ffmpeg.exe` and the `whisper\` folder next to it; without `whisper\`, SRec works but writes no subtitles.
