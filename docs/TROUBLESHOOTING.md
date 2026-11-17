# Troubleshooting and FAQ

Most problems leave a hint on the island or in `Videos\SRec Recordings\logs`. If none of these answers help, [open an issue](https://github.com/YOUR-GITHUB-USERNAME/SRec/issues/new/choose) and attach the log.

## Installing

**"Windows protected your PC" when running the setup.**
SmartScreen warns about apps without a code-signing certificate. Click **More info → Run anyway**. You can check the file against `SHA256SUMS.txt` first; see [Installation](INSTALLATION.md#2-download).

**My antivirus flags SRec.**
New unsigned apps sometimes get false positives. Check the SHA-256 hash, and please report the false positive to your antivirus vendor and in an issue.

## Starting and encoders

**The island says "No AV1 encoder – click for details".**
None of the AV1 encoders passed the test. Update your graphics driver, then open settings and click **Encoder** to re-test. Every PC can at least use SVT-AV1 on the CPU, so this usually means the capture itself failed. Remote Desktop sessions, for example, need GDI capture, which is tried automatically.

**"ffmpeg.exe missing – please reinstall SRec".**
`ffmpeg.exe` must be next to `srec.exe`. Reinstall, or for portable use copy the whole folder.

**It picked the CPU encoder (SVT-AV1) but I have an RTX 40 / RX 7000 / Arc GPU.**
Update the GPU driver and re-test the encoder in settings. On laptops with two GPUs, Windows may run SRec on the integrated one. If so, set *srec.exe* to *High performance* under **Settings → System → Display → Graphics**.

## Recording

**The recording is black or frozen.**
Some protected content (DRM video, some games in exclusive fullscreen) can't be captured by any screen recorder. For games, use borderless windowed mode.

**"Recording stopped unexpectedly".**
FFmpeg exited. The message names a log file; please attach it to a bug report. Recordings up to that moment are kept as MKV.

**The file is larger than I expected.**
The size target is a ceiling, and fast motion uses more of it. Choose a lower target, 30 fps instead of 60, or *Max 1080p*.

**The mouse pointer is missing or the island shows in my video.**
The pointer is always recorded. The island is hidden from captures on Windows 10 2004+. If it shows up anyway, please report your Windows version.

## Audio

**My voice is missing.**
1. Allow microphone access: **Settings → Privacy & security → Microphone → Let desktop apps access your microphone**.
2. Check that the mic button on the island isn't crossed out.
3. Check **Microphone** in SRec settings, and check Windows' *Sound settings* input level.
4. While recording, hover the island and speak: the green bar under the mic should move.

**My voice is too quiet / PC sound is too loud.**
SRec boosts the mic automatically and lowers PC sound while you speak. Built-in laptop mics are far from your mouth; a headset mic gives the best results.

**PC sound changes with my speaker volume.**
That happens on Windows 10 builds older than 20348, which lack *process loopback*. Windows 11 records PC sound at its real level.

**PC sound is in the recording although the speaker button is off.**
SRec isn't recording it digitally. If your laptop speakers play out loud, the **microphone hears them**. Use headphones.

**Audio and video drift apart.**
They shouldn't: SRec times audio on the wall clock. If you see drift, please report it with the recording length and your encoder.

## Subtitles

**No `.srt` file was created.**
- No speech was found (the island says *No speech found*), or
- subtitles were off: check that **CC** isn't crossed out, or
- the `whisper` folder is missing (settings show *Subtitles: Not installed*). Reinstall.

**"Subtitles failed. Log: …"**
Please attach that `.subtitles.log` to a bug report.

**The subtitles have mistakes.**
They're machine-generated. Clear speech, a closer mic and less background music all help. Only English is supported.

**VLC doesn't show the subtitles.**
The `.srt` must be next to the video with the same name. In VLC use **Subtitle → Sub Track** or **Subtitle → Add Subtitle File…**.

**Does subtitling slow down my PC?**
Very little while recording: at most 4 threads at below-normal priority, and only while there's sound to transcribe. After you stop, the last piece uses more threads for a few seconds so it finishes quickly.

## Privacy

**Does SRec send anything to the internet?**
No. The app has no network code. FFmpeg receives audio through a local pipe, and whisper.cpp runs on your PC. There's no telemetry or auto-update.

**Where are my settings?** `%APPDATA%\SRec\config.ini`. Delete it to reset SRec.
