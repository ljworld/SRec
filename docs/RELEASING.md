# Releasing

## Versioning

SRec follows [Semantic Versioning](https://semver.org). The version lives in **one place**, `version` in `Cargo.toml`. The build script and CI pass it to the installer.

| Stage | Version / tag | GitHub |
|---|---|---|
| Alpha (feature-complete, wide testing) | `1.0.0-alpha.1`, `-alpha.2`, … | Pre-release |
| Release candidate (no known blockers) | `1.0.0-rc.1`, … | Pre-release |
| Final | `1.0.0` | Latest release |
| Bug fixes / features afterwards | `1.0.1` / `1.1.0` | Latest release |

Windows file-version fields must be numeric, so the installer uses `1.0.0` for those and the full `1.0.0-alpha.1` for display and file names.

## Path to 1.0

1. **`1.0.0-alpha.1`** (current): publish as a pre-release and invite testers.
2. Work through [the test checklist](TESTING.md#2-pre-release-test-checklist) on at least two PCs. Fix issues, then release `-alpha.2` and so on as needed.
3. When the checklist passes and no *bug* issues are open for the milestone, release **`1.0.0-rc.1`**.
4. If nothing new is found after about a week of use, release **`1.0.0`**.

## Release steps

1. Update `version` in `Cargo.toml`, run `cargo build` so `Cargo.lock` follows, and add a section to `CHANGELOG.md`.
2. Run the quality checks locally:
   ```powershell
   cargo fmt --check; cargo clippy --release --all-targets -- -D warnings; cargo test --release
   ```
3. Build and smoke-test the installer locally: `.\build.ps1`, then install `dist\SRec-Setup-<version>.exe`.
4. Commit, tag and push:
   ```powershell
   git commit -am "Release 1.0.0-alpha.1"
   git tag v1.0.0-alpha.1
   git push origin main --tags
   ```
5. GitHub Actions builds everything and publishes the Release with:
   - `SRec-Setup-<version>.exe`
   - `SHA256SUMS.txt`
   - `ffmpeg-source-<commit>.tar.gz` (the exact FFmpeg source, required by the LGPL)
6. Edit the release notes on GitHub: paste the changelog section and add known issues.
7. Upload the setup file to [VirusTotal](https://www.virustotal.com) and link the result in the notes. Report any false positives to the vendors and to [Microsoft](https://www.microsoft.com/wdsi/filesubmission).

## Updating bundled components

| Component | Where it's pinned | How to update |
|---|---|---|
| FFmpeg | `FFMPEG_URL` in `build.ps1` and the workflow | Change the URL. CI re-checks the LGPL build and the required encoders |
| whisper.cpp | `b5454` URLs and SHA-256 in `build.ps1` and the workflow | Download the new `whisper-bin-x64.zip`, compute `Get-FileHash`, and update both files and `THIRD-PARTY-NOTICES.txt` |
| Speech model | `ggml-small.en-q5_1.bin` (+ hash) and `MODEL` in `src/subtitles.rs` | As above; update the cache key in the workflow |
| Rust crates | `Cargo.toml` (exact `=` versions for UI/audio/Windows crates) | Dependabot opens PRs weekly; test before merging |
