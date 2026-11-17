# Contributing to SRec

Thanks for helping! Contributions of every size are welcome: test reports on your hardware, bug reports, documentation fixes, translations of the docs, and code.

By taking part you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## Ways to help

- **Test the alpha.** Work through [docs/TESTING.md](docs/TESTING.md) on your PC and post the results in the release issue. Results from different GPUs (NVIDIA / AMD / Intel) are especially valuable.
- **Report bugs** with the *Bug report* template. Include:
  - your Windows build
  - your GPU and driver
  - the encoder shown in SRec's settings
  - the log from `Videos\SRec Recordings\logs`, if there is one
- **Suggest features** with the *Feature request* template. Describe the problem first, then your idea.
- **Improve the docs:** typos, clearer steps, screenshots.
- **Write code:** look for issues labelled `good first issue` or `help wanted`, or open an issue to discuss a bigger change before you start.

## Development setup

See [docs/BUILDING.md](docs/BUILDING.md). In short: Rust stable (MSVC) and the VS C++ Build Tools, then:

```powershell
powershell -ExecutionPolicy Bypass -File .\build.ps1   # first time: builds and fetches FFmpeg + Whisper
```

Read [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) to find your way around the code.

## Pull requests

1. Fork the repository and create a branch from `main`, for example `fix/mic-level-meter` or `feat/hotkey-settings`.
2. Keep each PR focused on one change. Small PRs get reviewed faster.
3. Before pushing, run the same checks as CI:
   ```powershell
   cargo fmt
   cargo clippy --release --all-targets -- -D warnings
   cargo test --release
   ```
4. Add or update unit tests for logic you change (audio maths, parsing, settings…). For hardware-dependent behaviour, describe how you tested it manually in the PR.
5. Update the docs (`README.md`, `docs/USER_MANUAL.md`) if users will notice the change, and add a line under **Unreleased** in [CHANGELOG.md](CHANGELOG.md).
6. Open the PR and fill in the template. CI must be green before merging.

### Code style

- `rustfmt` with the repository's `rustfmt.toml` (120 columns).
- No new `clippy` warnings.
- Comments explain *why*, not *what*. Keep them short and plain.
- Prefer failing soft: optional features (subtitles, audio sources) must never stop a recording. Show a warning on the island instead.
- **No network access.** SRec is offline by design. PRs that add telemetry, update checks or any network calls won't be accepted.
- New dependencies need a reason in the PR. They must be MIT/Apache-2.0/BSD-compatible (see `about.toml`), and must not be GPL.

### Commit messages

Use the imperative mood with a short summary line, for example `Fix mic gain pumping on silence`. Reference issues with `Fixes #123`.

## Licensing of contributions

SRec is MIT licensed. By submitting a contribution you agree that it's licensed under the [MIT License](LICENSE), and that you have the right to submit it.

## Questions

Use [GitHub Discussions](https://github.com/YOUR-GITHUB-USERNAME/SRec/discussions) for questions and ideas. Issues are for bugs and concrete feature requests.
