//! Offline English subtitles, written live while recording.
//!
//! The audio mixer hands this module the recording's audio (voice + PC
//! sound) as 16 kHz mono. It is cut into ~15 s pieces at a quiet moment, so
//! words are not split, and each piece is transcribed by the bundled
//! whisper.cpp while the recording goes on. When recording stops only the
//! last piece is left, so "<video name>.srt" is ready a few seconds after
//! the video. VLC, MPC-HC, PotPlayer and Windows Media Player load a
//! same-named .srt automatically.
//!
//! CPU use is kept low while recording: few threads, below-normal priority,
//! and silent pieces are skipped without running whisper at all. Everything
//! runs locally; no audio ever leaves the PC.

use crate::encoder::CREATE_NO_WINDOW;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;

/// Keeps whisper from slowing down the PC or the recording.
const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
const MODEL: &str = "ggml-small.en-q5_1.bin";
const VAD_MODEL: &str = "ggml-silero-v6.2.0.bin";
/// Longest subtitle line, in characters.
const MAX_LINE: &str = "64";

const SR: usize = 16_000;
/// A piece is cut at the quietest moment between these two lengths (~15 s).
const MIN_PIECE: usize = 12 * SR;
const MAX_PIECE: usize = 17 * SR;
/// Pieces whose loudest sample is below this (~ -66 dBFS) are silence.
const SILENT_PEAK: f32 = 0.0005;
/// Previous text handed to whisper so sentences continue across pieces.
const PROMPT_CHARS: usize = 200;

#[derive(Clone)]
pub struct Whisper {
    cli: PathBuf,
    model: PathBuf,
    vad: Option<PathBuf>,
}

/// Looks for `whisper\whisper-cli.exe` and the model next to srec.exe.
pub fn find() -> Option<Whisper> {
    let dir = std::env::current_exe().ok()?.parent()?.join("whisper");
    let cli = dir.join("whisper-cli.exe");
    let model = dir.join(MODEL);
    if !cli.is_file() || !model.is_file() {
        return None;
    }
    let vad = Some(dir.join(VAD_MODEL)).filter(|p| p.is_file());
    Some(Whisper { cli, model, vad })
}

pub enum Event {
    /// Subtitles complete; `lines` = number of subtitle entries.
    Done {
        lines: usize,
    },
    NoSpeech,
    Failed(String),
}

/// Starts live subtitles for one recording. Feed 16 kHz mono audio into the
/// returned sender; dropping it (recording stopped) finishes the file and
/// sends one `Event`.
pub fn start_live(
    w: Whisper,
    srt: PathBuf,
    log: PathBuf,
    ctx: eframe::egui::Context,
) -> (mpsc::Sender<Vec<f32>>, mpsc::Receiver<Event>) {
    let (tx, rx) = mpsc::channel::<Vec<f32>>();
    let (etx, erx) = mpsc::channel();
    // If the thread can't start, recording still works; the audio sent to
    // `tx` is simply dropped.
    let _ = std::thread::Builder::new().name("subtitles".into()).spawn(move || {
        clean_temp();
        let ev = Live::new(w, srt, log).run(rx);
        let _ = etx.send(ev);
        ctx.request_repaint();
    });
    (tx, erx)
}

struct Cue {
    start: u64, // ms
    end: u64,
    text: Vec<String>,
}

struct Live {
    w: Whisper,
    srt: PathBuf,
    log: PathBuf,
    vad: bool,
    cues: Vec<Cue>,
    prompt: String,
    pieces: usize,
    ok_runs: usize,
    failed_runs: usize,
}

impl Live {
    fn new(w: Whisper, srt: PathBuf, log: PathBuf) -> Self {
        let vad = w.vad.is_some();
        Self {
            w,
            srt,
            log,
            vad,
            cues: Vec::new(),
            prompt: String::new(),
            pieces: 0,
            ok_runs: 0,
            failed_runs: 0,
        }
    }

    fn run(mut self, rx: mpsc::Receiver<Vec<f32>>) -> Event {
        let mut buf: Vec<f32> = Vec::with_capacity(MAX_PIECE + SR);
        let mut buf_start: usize = 0; // position of buf[0] in the recording, samples
        let mut open = true;
        loop {
            if open {
                match rx.recv() {
                    Ok(v) => buf.extend_from_slice(&v),
                    Err(_) => open = false,
                }
                while let Ok(v) = rx.try_recv() {
                    buf.extend_from_slice(&v);
                }
            }
            while buf.len() >= MAX_PIECE || (!open && !buf.is_empty()) {
                let cut = if buf.len() >= MAX_PIECE {
                    quiet_cut(&buf)
                } else {
                    buf.len()
                };
                let piece: Vec<f32> = buf.drain(..cut).collect();
                // While recording, stay light; once stopped, finish quickly.
                self.piece(&piece, buf_start, open);
                buf_start += cut;
            }
            if !open {
                break;
            }
        }
        self.finish()
    }

    fn piece(&mut self, samples: &[f32], offset: usize, recording: bool) {
        let peak = samples.iter().fold(0f32, |m, x| m.max(x.abs()));
        if samples.len() < SR / 3 || peak < SILENT_PEAK {
            return; // nothing to say - don't wake whisper
        }
        self.pieces += 1;
        let base = std::env::temp_dir().join(format!("srec-live-{}-{}", std::process::id(), self.pieces));
        let wav = base.with_extension("wav");
        let out = base.with_extension("srt");
        if write_wav(&wav, samples).is_err() {
            self.failed_runs += 1;
            return;
        }
        let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        let threads = if recording {
            (cores / 4).clamp(2, 4)
        } else {
            (cores / 2).clamp(2, 8)
        };
        let mut ok = self.whisper(&wav, &base, threads, self.vad);
        if !ok && self.vad {
            // Voice detection is an optional extra; carry on without it.
            self.vad = false;
            ok = self.whisper(&wav, &base, threads, false);
        }
        let text = if ok { fs::read_to_string(&out).ok() } else { None };
        let _ = fs::remove_file(&wav);
        let _ = fs::remove_file(&out);
        let Some(text) = text else {
            self.failed_runs += 1;
            return;
        };
        self.ok_runs += 1;

        let off = (offset as u64 * 1000) / SR as u64;
        let piece_end = off + (samples.len() as u64 * 1000) / SR as u64;
        let mut added = false;
        for mut c in parse_srt(&text) {
            c.start = (c.start + off).min(piece_end);
            c.end = (c.end + off).min(piece_end);
            if let Some(prev) = self.cues.last() {
                c.start = c.start.max(prev.end);
            }
            if c.end <= c.start {
                continue;
            }
            self.prompt.push(' ');
            self.prompt.push_str(&c.text.join(" "));
            self.cues.push(c);
            added = true;
        }
        if self.prompt.chars().count() > PROMPT_CHARS {
            let skip = self.prompt.chars().count() - PROMPT_CHARS;
            self.prompt = self.prompt.chars().skip(skip).collect();
        }
        if added {
            // Saved after every piece, so the file is usable even if SRec
            // or the PC stops unexpectedly.
            let _ = fs::write(&self.srt, render_srt(&self.cues));
        }
    }

    fn whisper(&self, wav: &Path, base: &Path, threads: usize, vad: bool) -> bool {
        let Ok(mut logf) = OpenOptions::new().create(true).append(true).open(&self.log) else {
            return false;
        };
        let _ = writeln!(logf, "--- piece {} ---", self.pieces);
        let Ok(logf) = logf.try_clone() else { return false };
        let mut cmd = Command::new(&self.w.cli);
        cmd.arg("-m")
            .arg(&self.w.model)
            .arg("-f")
            .arg(wav)
            .args(["-l", "en", "-t"])
            .arg(threads.to_string())
            .args(["-osrt", "-np", "-sow", "-ml", MAX_LINE, "-of"])
            .arg(base);
        let prompt = self.prompt.trim();
        if !prompt.is_empty() {
            cmd.arg("--prompt").arg(prompt);
        }
        if let (true, Some(v)) = (vad, &self.w.vad) {
            cmd.arg("--vad").arg("-vm").arg(v);
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(logf))
            .creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn finish(self) -> Event {
        if !self.cues.is_empty() {
            let _ = fs::write(&self.srt, render_srt(&self.cues));
            if self.failed_runs == 0 {
                let _ = fs::remove_file(&self.log);
            }
            return Event::Done { lines: self.cues.len() };
        }
        let _ = fs::remove_file(&self.srt);
        if self.failed_runs > 0 && self.ok_runs == 0 {
            return Event::Failed(format!("Subtitles failed. Log: {}", self.log.display()));
        }
        let _ = fs::remove_file(&self.log);
        Event::NoSpeech
    }
}

/// Index (in samples) of the quietest 20 ms between MIN_PIECE and MAX_PIECE.
fn quiet_cut(buf: &[f32]) -> usize {
    const FRAME: usize = SR / 50;
    let end = buf.len().min(MAX_PIECE);
    let mut best = (f32::MAX, end);
    let mut i = MIN_PIECE;
    while i + FRAME <= end {
        let e: f32 = buf[i..i + FRAME].iter().map(|x| x * x).sum();
        if e < best.0 {
            best = (e, i + FRAME / 2);
        }
        i += FRAME;
    }
    best.1
}

fn write_wav(path: &Path, samples: &[f32]) -> std::io::Result<()> {
    let data_len = (samples.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + data_len as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes()); // PCM
    b.extend_from_slice(&1u16.to_le_bytes()); // mono
    b.extend_from_slice(&(SR as u32).to_le_bytes());
    b.extend_from_slice(&(SR as u32 * 2).to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for x in samples {
        b.extend_from_slice(&((x.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    File::create(path)?.write_all(&b)
}

/// Removes temporary audio left behind if SRec was closed mid-way.
fn clean_temp() {
    let Ok(rd) = fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    for e in rd.flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        if n.starts_with("srec-") && (n.ends_with(".wav") || n.ends_with(".srt")) {
            let _ = fs::remove_file(e.path()); // fails harmlessly if in use
        }
    }
}

fn parse_time(s: &str) -> Option<u64> {
    let s = s.trim().replace('.', ",");
    let (hms, ms) = s.split_once(',')?;
    let mut p = hms.split(':');
    let h: u64 = p.next()?.parse().ok()?;
    let m: u64 = p.next()?.parse().ok()?;
    let sec: u64 = p.next()?.parse().ok()?;
    let ms: u64 = ms.parse().ok()?;
    Some(((h * 60 + m) * 60 + sec) * 1000 + ms)
}

fn fmt_time(ms: u64) -> String {
    format!(
        "{:02}:{:02}:{:02},{:03}",
        ms / 3_600_000,
        (ms / 60_000) % 60,
        (ms / 1000) % 60,
        ms % 1000
    )
}

/// Reads whisper's SRT, dropping "[BLANK_AUDIO]", "(music)" and similar.
fn parse_srt(text: &str) -> Vec<Cue> {
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let mut out = Vec::new();
    for block in text.split("\n\n") {
        let mut it = block.lines().map(str::trim).filter(|l| !l.is_empty());
        let Some(first) = it.next() else { continue };
        let time = if first.contains("-->") {
            first
        } else {
            match it.next() {
                Some(t) if t.contains("-->") => t,
                _ => continue,
            }
        };
        let Some((a, b)) = time.split_once("-->") else { continue };
        let (Some(start), Some(end)) = (parse_time(a), parse_time(b)) else {
            continue;
        };
        let lines: Vec<String> = it.map(str::to_string).collect();
        if !is_speech(&lines.join(" ")) {
            continue;
        }
        out.push(Cue {
            start,
            end,
            text: lines,
        });
    }
    out
}

/// UTF-8 with BOM and CRLF: what Windows media players handle best.
fn render_srt(cues: &[Cue]) -> String {
    let mut s = String::from("\u{feff}");
    for (i, c) in cues.iter().enumerate() {
        s.push_str(&format!(
            "{}\r\n{} --> {}\r\n{}\r\n\r\n",
            i + 1,
            fmt_time(c.start),
            fmt_time(c.end),
            c.text.join("\r\n")
        ));
    }
    s
}

/// False for lines that are only non-speech tags like "[BLANK_AUDIO]",
/// "(upbeat music)" or "*laughs*".
fn is_speech(s: &str) -> bool {
    let mut depth = 0i32;
    let mut stars = false;
    let mut words = String::new();
    for c in s.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = (depth - 1).max(0),
            '*' => stars = !stars,
            _ if depth == 0 && !stars => words.push(c),
            _ => {}
        }
    }
    words.chars().any(|c| c.is_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_drops_non_speech() {
        let src = "1\n00:00:00,000 --> 00:00:02,000\n[BLANK_AUDIO]\n\n\
                   2\n00:00:02,000 --> 00:00:04,500\n Hello and welcome.\n\n\
                   3\n00:00:05,000 --> 00:00:06,000\n(upbeat music)\n\n\
                   4\n00:01:06,250 --> 00:01:08,000\n Today we look at SRec.\n";
        let c = parse_srt(src);
        assert_eq!(c.len(), 2);
        assert_eq!((c[0].start, c[0].end), (2000, 4500));
        assert_eq!(c[1].start, 66_250);
        assert_eq!(c[1].text, vec!["Today we look at SRec.".to_string()]);
    }

    #[test]
    fn renders_numbered_srt() {
        let cues = vec![
            Cue {
                start: 2000,
                end: 4500,
                text: vec!["Hello.".into()],
            },
            Cue {
                start: 3_723_004,
                end: 3_725_000,
                text: vec!["Later.".into()],
            },
        ];
        let s = render_srt(&cues);
        assert!(s.starts_with("\u{feff}1\r\n00:00:02,000 --> 00:00:04,500\r\nHello.\r\n\r\n"));
        assert!(s.contains("2\r\n01:02:03,004 --> 01:02:05,000\r\nLater.\r\n"));
    }

    #[test]
    fn cuts_at_the_quiet_moment() {
        let mut buf = vec![0.3f32; MAX_PIECE + 100];
        let quiet = 14 * SR;
        for x in &mut buf[quiet..quiet + SR / 10] {
            *x = 0.0;
        }
        let cut = quiet_cut(&buf);
        assert!(cut >= quiet && cut < quiet + SR / 10, "cut at {cut}");
        // No pause at all: still cut within the 12-17 s window.
        let loud = vec![0.3f32; MAX_PIECE];
        let c = quiet_cut(&loud);
        assert!((MIN_PIECE..=MAX_PIECE).contains(&c));
    }

    #[test]
    fn wav_header_is_valid() {
        let p = std::env::temp_dir().join("srec-test-header.wav");
        write_wav(&p, &[0.0, 0.5, -0.5]).unwrap();
        let b = fs::read(&p).unwrap();
        let _ = fs::remove_file(&p);
        assert_eq!(&b[0..4], b"RIFF");
        assert_eq!(b.len(), 44 + 6);
        assert_eq!(u32::from_le_bytes([b[24], b[25], b[26], b[27]]), 16_000);
    }

    #[test]
    fn speech_detection() {
        assert!(is_speech("So [inaudible] this works"));
        assert!(!is_speech("[Music]"));
        assert!(!is_speech(" *laughs* "));
        assert!(!is_speech("♪ ♪"));
    }
}
