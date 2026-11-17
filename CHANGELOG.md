# Changelog

All notable changes to SRec are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [1.0.0-alpha.1] - 2026-11-17

First public pre-release, under the new name **SRec**.

### Added
- **Live English subtitles:** whisper.cpp (Whisper `small.en`) transcribes the recording in ~15-second pieces while you record, cutting at natural pauses. The `.srt` is saved next to the video a few seconds after it, and VLC and other players load it automatically. Fully offline.
  - Silero voice-activity detection skips silence and music.
  - `[BLANK_AUDIO]`, `(music)` and similar tags are removed.
  - Low CPU use while recording: at most 4 threads at below-normal priority, and silent pieces are skipped.
- **CC button** on the island and a *Subtitles* setting to switch subtitles on or off, also during a recording.
- **PC sound at its real level:** Windows process loopback records apps' sound before the speaker volume is applied. Classic loopback is the fallback on older Windows 10 builds.
- **Clear voice:** automatic microphone gain (up to +20 dB, held in pauses) and ducking of PC sound while you speak.
- Warnings on the island when the microphone is unavailable or silent (Windows privacy setting).
- Developer and user documentation, issue/PR templates, Dependabot, and a CI quality gate (`fmt`, `clippy -D warnings`, unit tests).
- Pre-release builds: tags such as `v1.0.0-alpha.1` are published as GitHub pre-releases.

### Changed
- Renamed from **AV1 Island** to **SRec**. The installer upgrades the old install and migrates its settings.
- Recordings are saved to `Videos\SRec Recordings` for new installs; existing users keep their folder.

### Fixed
- **Lost audio:** a repeating pattern of about 2 s of sound and 8 s of silence, which also lost the microphone. The mixer no longer waits when FFmpeg pauses reading its audio pipe. Audio is queued and nothing is dropped; device buffers now hold 10 s.
- The end of the recording's audio is no longer cut off when stopping.

## [0.3.0] - 2026-11-09

Last release as **AV1 Island**: AV1 screen recording with automatic GPU encoder selection, zero-copy Desktop Duplication capture, hourly size targets, window/area picker, mic + speaker mixing, and an offline installer.

[Unreleased]: https://github.com/ljworld/SRec/compare/v1.0.0-alpha.1...HEAD
[1.0.0-alpha.1]: https://github.com/ljworld/SRec/releases/tag/v1.0.0-alpha.1
