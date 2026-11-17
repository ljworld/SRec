# SRec user manual

- [1. The island](#1-the-island)
- [2. Recording](#2-recording)
- [3. Choosing what to record](#3-choosing-what-to-record)
- [4. Audio](#4-audio)
- [5. Subtitles](#5-subtitles)
- [6. Settings](#6-settings)
- [7. Your files](#7-your-files)
- [8. Choosing a size target](#8-choosing-a-size-target)
- [9. Keyboard shortcuts](#9-keyboard-shortcuts)
- [10. Tips](#10-tips)

---

## 1. The island

SRec is a small dark pill ("island") at the top of the screen. It's always on top, but **it never appears in your recordings**: Windows hides it from screen capture.

**Collapsed**, it shows a status dot and a status line:

| Dot | Meaning |
|---|---|
| Green | Ready to record |
| Red, pulsing | Recording |
| Amber, pulsing | Busy (checking the graphics card or saving) |
| Amber | Needs attention (click the status text for details) |

**Hover over it** and it expands to show the controls, from left to right:

| Control | What it does |
|---|---|
| ● Record / ■ Stop | Starts or stops recording (`Ctrl+Shift+R`) |
| Frame | Choose a window or area to record (`Ctrl+Shift+S`) |
| Microphone | Mic on/off. While recording, a green bar under it shows your voice level |
| Speaker | PC sound on/off. When off, no PC sound goes into the video or the subtitles |
| **CC** | Live subtitles on/off |
| Gear | Opens or closes the settings panel |
| ✕ | Quits SRec (disabled while recording; stop first) |

**To move the island**, drag it with the mouse. **Clicking the status text** after a recording shows the file in Explorer.

## 2. Recording

1. Check the status line. For example, `Quick Sync AV1 · Full screen · 700 MB/h` means:
   - Quick Sync AV1 is the encoder
   - the full screen will be recorded
   - 700 MB per hour is the size target
2. Press **`Ctrl+Shift+R`** or click **●**. The dot turns red.
3. While recording, the status line shows the elapsed time and the current file size. After 20 seconds it also shows the projected size per hour, for example `00:05:12   18.4 MB   ~212 MB/h`.
4. Press **`Ctrl+Shift+R`** again or click **■** to stop.
5. The island shows *Saving MP4...* for a moment, then *Saved 18.4 MB · subtitles: 42 lines – click to show*.

You can turn the mic, the PC sound and subtitles on or off **during** a recording.

> SRec records to MKV first, which survives crashes and power loss, and converts it to MP4 in a few seconds when you stop, without re-encoding. If conversion fails, the MKV is kept. Every player that handles MP4 also plays it.

## 3. Choosing what to record

Press **`Ctrl+Shift+S`** or click the **frame** icon. The screen dims, and then:

| Action | Records |
|---|---|
| **Click a window** | That window's area |
| **Drag a rectangle** | That area |
| **Right-click** | The whole display under the pointer |
| **Esc** | Cancel |

The status line then shows `Window: …`, `Area 1280 x 720 at …` or `Display 2`. To go back to the full screen, open settings and click the **Capture** row.

Notes:
- **Window capture** records the window's area of the screen at the moment recording starts. If you move the window, the recorded area stays where it was. Windows on top of it are recorded too.
- An area or window must be on **one monitor**. It's clipped to the monitor that holds its centre.
- With several monitors, the **Display** setting chooses which one *Full screen* means.

## 4. Audio

SRec records two sources and mixes them into one audio track:

- **Microphone:** your voice, from the Windows default mic or the one chosen in settings.
  - **Automatic gain** lifts quiet laptop microphones by up to +20 dB. During pauses the gain is held, so background hiss isn't pumped up.
  - **Ducking:** PC sound is lowered by about 9 dB while you speak, so your voice stays clear.
- **PC sound:** everything your apps play.
  - On Windows 11 and Windows 10 build 20348+, it's recorded **at its real level, regardless of the speaker volume**. You can turn your speakers down without making the recording quieter.
  - Individual app volumes in the Windows *Volume mixer* still apply.

The **mic** and **speaker** buttons switch each source on or off, at any time.

> **Laptop speakers and the mic:** if you play sound out loud, the laptop microphone hears it too. To keep PC sound out of the recording entirely, switch PC sound off **and** use headphones.

If the mic is blocked by Windows privacy settings, the island warns: *Microphone is silent – check Settings > Privacy & security > Microphone*.

## 5. Subtitles

SRec writes **English subtitles** while you record, using whisper.cpp and the Whisper `small.en` speech model on your own PC.

- **What's transcribed:** the recorded audio, meaning your voice and PC sound, only while the speaker button is on. Only English speech is supported.
- **When:** live, in pieces of about 15 seconds. Each cut is placed at a natural pause, so words aren't split. When you stop, only the last piece is left, so the `.srt` is ready a few seconds after the video.
- **Where:** `Recording_<date>_<time>.srt`, next to the video, with the same name.
- **Playback:** VLC, MPC-HC, PotPlayer and Windows Media Player load a same-named `.srt` automatically. In VLC you can also use **Subtitle → Add Subtitle File…**. YouTube and most video editors import `.srt` too.
- **CPU use:** at most 4 threads at below-normal priority while recording, so your apps and the recording come first. Silent pieces are skipped without running Whisper at all.
- **On/off:** click **CC** on the island, or **Subtitles** in settings.
  - Turning it off during a recording pauses subtitles from that moment.
  - Turning it on during a recording that started without subtitles takes effect from the next recording.

Silence, music and noises like `[BLANK_AUDIO]` or `(music)` are filtered out. If nobody speaks, no `.srt` is created and the island says so.

> Subtitles are machine-generated and can contain mistakes, especially with music in the background, strong accents or several people talking at once.

## 6. Settings

Click the **gear** to open the panel. Click a row to change it. Most rows are locked while recording; Subtitles and Recordings always work.

| Setting | Options | Notes |
|---|---|---|
| Graphics | – | Detected graphics cards and video memory |
| Encoder | Click to re-test | The AV1 encoder in use. Re-test after a driver update or a GPU change |
| Capture | Click for full display | Current window or area selection |
| Size target | 400 MB · 500 MB · 700 MB · 1 GB · 1.5 GB per hour | Maximum file size per hour. See [section 8](#8-choosing-a-size-target) |
| Resolution | Max 1080p / Native | Max 1080p scales larger screens down to fit 1920×1080, giving smaller files |
| Frame rate | 15 · 25 · 30 · 60 fps | 30 suits most content; use 60 for games or smooth scrolling |
| Display | 1 of N | Which monitor *Full screen* means |
| Microphone | System default, or a named device | Click to cycle through microphones |
| Subtitles | On / Off | Same as the **CC** button |
| Recordings | (open) | Opens the recordings folder |
| Hotkeys | – | Reminder of the shortcuts |
| About | (licences) | Version. Opens the folder with the licence files |

Settings are saved immediately to `%APPDATA%\SRec\config.ini`.

## 7. Your files

All in `%USERPROFILE%\Videos\SRec Recordings`:

| File | What |
|---|---|
| `Recording_2026-11-17_09-30-00.mp4` | The video: AV1 video with Opus audio |
| `Recording_2026-11-17_09-30-00.srt` | English subtitles (if speech was found) |
| `Recording_…mkv` | Only present if MP4 conversion failed; plays fine in VLC |
| `logs\Recording_….log` | FFmpeg warnings; deleted when empty |
| `logs\Recording_….subtitles.log` | Whisper output; kept only if subtitling had errors |

**Playing AV1:** Windows 11 plays AV1 in the *Media Player* app. On Windows 10, install the free *AV1 Video Extension* from the Microsoft Store, or use VLC. Browsers, YouTube and modern editors (DaVinci Resolve, Premiere Pro, Shotcut) handle AV1 too.

## 8. Choosing a size target

The size target is a **hard ceiling**. SRec aims for constant visual quality, so calm screens use much less than the ceiling.

| Target | Typical use | Notes |
|---|---|---|
| 400 MB/h | Meetings, slides | Smallest files |
| 500 MB/h | Office work, documents | |
| **700 MB/h** | **Tutorials at 1080p (default)** | Typical real use: 100–300 MB/h |
| 1 GB/h | Videos playing on screen | |
| 1.5 GB/h | Games, fast motion | Combine with 60 fps |

## 9. Keyboard shortcuts

These work from any app, even when the island isn't focused:

| Shortcut | Action |
|---|---|
| `Ctrl+Shift+R` | Start / stop recording |
| `Ctrl+Shift+S` | Choose window / area |
| `Esc` (in the picker) | Cancel picking |

## 10. Tips

- **Recording a tutorial?** Use *Max 1080p*, 30 fps and 700 MB/h, with the mic on and subtitles on.
- **Recording a meeting?** Use 400 MB/h and 15 or 25 fps. Keep PC sound on so the other people are recorded and subtitled. Always get consent first; see the Terms of Use.
- **Voice too quiet?** Get closer to the mic, or choose a headset mic in settings. The automatic gain does the rest.
- **Long recordings** are fine: the MKV is crash-safe and the subtitles are saved every ~15 seconds.
