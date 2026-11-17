# Testing

SRec talks to GPUs, audio devices and helper programs, so it's tested in two layers:
- **automated unit tests**, which run in CI on every push
- a **manual release checklist**, worked through on real hardware before every alpha, release candidate and final release

## 1. Automated tests

```powershell
cargo test --release
```

| Area | Tests |
|---|---|
| Voice processing | Quiet laptop speech is boosted; loud speech isn't; background noise isn't pumped up; ducking engages while speaking and releases afterwards |
| Mixing | PC sound is fully excluded when the speaker button is off |
| Subtitles | SRT parsing, non-speech cues dropped, renumbering and time formatting, cuts land in the quiet moment, WAV header, speech detection |
| Process loopback | Manual only (`SREC_LOOPBACK_SECS=4`, see [BUILDING.md](BUILDING.md#quality-checks-same-as-ci)) |

CI also enforces `cargo fmt --check` and `cargo clippy -D warnings`.

## 2. Pre-release test checklist

Copy this list into the release's tracking issue and tick it on at least **two different PCs**, ideally with different GPU vendors. Record the encoder shown in settings for each PC.

### Install
- [ ] Fresh install (per-user) on a PC without SRec: the SmartScreen flow works, the Terms page requires *I accept*, Start-menu shortcut is created
- [ ] Upgrade over the previous version keeps settings (`%APPDATA%\SRec\config.ini`)
- [ ] Optional: upgrade from *AV1 Island* 0.3.x removes the old exe, shortcuts and autostart entry
- [ ] *Start SRec when I sign in* works after a reboot

### First start
- [ ] Island appears top centre and shows *Checking graphics card...*, then *Ready*
- [ ] Settings → Graphics lists the GPU; Encoder shows the expected hardware encoder
- [ ] Re-test encoder works

### Video
- [ ] 1-minute full-screen recording: MP4 plays in VLC and in Windows Media Player
- [ ] The island isn't visible in the video
- [ ] Window capture (`Ctrl+Shift+S` → click a window) records just that area
- [ ] Area capture (drag) and right-click display capture work
- [ ] Second monitor (if available) records correctly
- [ ] *Max 1080p* on a larger-than-1080p screen gives 1920-wide output; *Native* gives full resolution
- [ ] 60 fps setting produces 60 fps (check with `ffprobe` or VLC *Codec info*)
- [ ] The projected `~MB/h` in the status stays under the size target
- [ ] CPU use during recording (Task Manager) is low with a hardware encoder

### Audio
- [ ] Voice is clear and continuous for a 5-minute recording (no gaps or repeats)
- [ ] PC sound (e.g. a YouTube video) is recorded; speaking lowers it
- [ ] Changing the **Windows speaker volume** mid-recording doesn't change the recorded PC-sound level (Windows 11)
- [ ] Mic button off mid-recording: voice disappears; on again: it returns
- [ ] Speaker button off: no PC sound in the file (use headphones to avoid acoustic pickup)
- [ ] Mic privacy switched off in Windows: the island warns *Microphone is silent* within ~3 s
- [ ] Audio stays in sync with the video at the end of a 30-minute recording

### Subtitles
- [ ] 2-minute English talk: `.srt` appears within ~10 s of *Saved*, and VLC shows it automatically
- [ ] Subtitle timing matches speech at the start, middle and end
- [ ] CC button off mid-recording pauses subtitles; the island shows the message
- [ ] A recording with only music or silence gives *No speech found* and no `.srt`
- [ ] CPU stays moderate while recording with subtitles on

### Robustness
- [ ] Kill `srec.exe` in Task Manager during recording: the MKV and the partial `.srt` remain playable
- [ ] Stop and immediately start a new recording: both files and both `.srt` files are complete
- [ ] Unplug the microphone during recording: recording continues
- [ ] Close SRec while subtitles finish: no crash; the `.srt` exists

### Uninstall
- [ ] Uninstall removes the program and settings and keeps the recordings
- [ ] No `whisper-cli.exe` or `ffmpeg.exe` left running

## 3. Reporting results

Post the ticked checklist in the release issue, with each PC's CPU, GPU, driver version and Windows build, and the encoder SRec chose. Open separate bug reports for failures using the *Bug report* template.
