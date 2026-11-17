//! One recording session: FFmpeg process + audio engine, then remux to MP4.

use crate::audio::{self, AudioEngine, Controls};
use crate::encoder::{self, Pipeline, VideoParams, CREATE_NO_WINDOW};
use crate::platform;
use crate::subtitles::{self, Whisper};
use std::fs::{self, File};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

pub struct Saved {
    pub path: PathBuf,
    pub bytes: u64,
}

pub struct Session {
    child: Child,
    running: bool,
    audio: AudioEngine,
    pub ctl: Arc<Controls>,
    pub started: Instant,
    pub mkv: PathBuf,
    log: PathBuf,
    /// Live subtitles: reports once when the .srt is complete.
    pub subs: Option<mpsc::Receiver<subtitles::Event>>,
    subs_started: bool,
}

/// Audio sources for a recording.
pub struct AudioSetup {
    /// Microphone name, or None for the Windows default.
    pub mic: Option<String>,
    pub mic_on: bool,
    pub spk_on: bool,
    /// Live subtitles (whisper + UI context to wake on completion).
    pub subs: Option<(Whisper, eframe::egui::Context)>,
}

pub fn start(
    ffmpeg: &Path,
    p: Pipeline,
    v: &VideoParams,
    out_dir: &Path,
    audio_in: AudioSetup,
) -> Result<Session, String> {
    let AudioSetup {
        mic,
        mic_on,
        spk_on,
        subs,
    } = audio_in;
    let logs = out_dir.join("logs");
    fs::create_dir_all(&logs).map_err(|e| format!("Cannot create {}: {e}", out_dir.display()))?;
    let name = format!("Recording_{}", platform::timestamp());
    let mkv = out_dir.join(format!("{name}.mkv"));
    let log = logs.join(format!("{name}.log"));
    let logf = File::create(&log).map_err(|e| format!("Cannot write log: {e}"))?;

    let (vin, vf) = encoder::video_input(p, v);
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-hide_banner", "-nostdin", "-loglevel", "warning", "-y"])
        .args(&vin)
        .args([
            "-thread_queue_size",
            "4096",
            "-f",
            "f32le",
            "-ar",
            "48000",
            "-ac",
            "2",
            "-i",
            "pipe:0",
        ])
        // The audio pipe defines the length: closing it ends the file cleanly.
        .args(["-map", "0:v:0", "-map", "1:a:0", "-shortest"]);
    if let Some(f) = vf {
        cmd.args(["-vf", f.as_str()]);
    }
    cmd.args(encoder::video_encoder(p.enc, v))
        .args(encoder::audio_encoder(v))
        .args(["-f", "matroska"])
        .arg(&mkv)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::from(logf))
        .creation_flags(CREATE_NO_WINDOW);

    let mut child = cmd.spawn().map_err(|e| format!("Could not start FFmpeg: {e}"))?;
    let Some(stdin) = child.stdin.take() else {
        let _ = child.kill();
        return Err("Could not open the audio pipe to FFmpeg".into());
    };
    let ctl = Arc::new(Controls::new(mic_on, spk_on));
    // Same name as the video, so players pick it up for both .mkv and .mp4.
    let (sub_tx, sub_rx) = match subs {
        Some((w, ctx)) => {
            let srt = out_dir.join(format!("{name}.srt"));
            let slog = logs.join(format!("{name}.subtitles.log"));
            let (tx, rx) = subtitles::start_live(w, srt, slog, ctx);
            (Some(tx), Some(rx))
        }
        None => (None, None),
    };
    let audio = match audio::start(ctl.clone(), mic, stdin, sub_tx) {
        Ok(a) => a,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
    };
    Ok(Session {
        child,
        running: true,
        audio,
        ctl,
        started: Instant::now(),
        mkv,
        log,
        subs_started: sub_rx.is_some(),
        subs: sub_rx,
    })
}

impl Session {
    /// True if live subtitles were started for this recording.
    pub fn has_subtitles(&self) -> bool {
        self.subs_started
    }

    pub fn size(&self) -> u64 {
        fs::metadata(&self.mkv).map(|m| m.len()).unwrap_or(0)
    }

    /// Audio warnings raised since the last call (e.g. "Microphone unavailable").
    pub fn take_warnings(&self) -> Vec<String> {
        self.audio
            .warnings
            .lock()
            .map(|mut w| std::mem::take(&mut *w))
            .unwrap_or_default()
    }

    /// Returns an error message if FFmpeg died while recording.
    pub fn check_exited(&mut self) -> Option<String> {
        match self.child.try_wait() {
            Ok(Some(st)) => Some(format!(
                "Recording stopped unexpectedly ({st}). Log: {}",
                self.log.display()
            )),
            _ => None,
        }
    }

    fn halt(&mut self) {
        self.running = false;
        // Stopping the mixer closes FFmpeg's stdin; with -shortest FFmpeg then
        // flushes the encoders and finalises the file by itself.
        self.audio.stop();
        // Audio queued while FFmpeg was paused is still being fed to it.
        // Give it time to drain (up to 5 min), then 20 s to finalise the file.
        let t = Instant::now();
        let mut drained: Option<Instant> = None;
        loop {
            if drained.is_none() && (self.audio.writer_done() || t.elapsed() > Duration::from_secs(300)) {
                drained = Some(Instant::now());
            }
            let finishing = drained.map(|d| d.elapsed() < Duration::from_secs(20)).unwrap_or(true);
            match self.child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if finishing => std::thread::sleep(Duration::from_millis(100)),
                _ => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    break;
                }
            }
        }
    }

    /// Stop gracefully and convert to MP4 on a background thread.
    pub fn stop(self, ffmpeg: PathBuf) -> mpsc::Receiver<Result<Saved, String>> {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let r = self.finish(&ffmpeg);
            let _ = tx.send(r);
        });
        rx
    }

    fn finish(mut self, ffmpeg: &Path) -> Result<Saved, String> {
        self.halt();
        if fs::metadata(&self.mkv).map(|m| m.len()).unwrap_or(0) == 0 {
            return Err(format!("Nothing was recorded. Log: {}", self.log.display()));
        }
        let mp4 = self.mkv.with_extension("mp4");
        let st = Command::new(ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-i"])
            .arg(&self.mkv)
            .args(["-map", "0", "-c", "copy", "-movflags", "+faststart"])
            .arg(&mp4)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .status();
        let ok = matches!(st, Ok(s) if s.success()) && fs::metadata(&mp4).map(|m| m.len() > 0).unwrap_or(false);
        let path = if ok {
            let _ = fs::remove_file(&self.mkv);
            mp4
        } else {
            self.mkv.clone()
        };
        if fs::metadata(&self.log).map(|m| m.len() == 0).unwrap_or(false) {
            let _ = fs::remove_file(&self.log);
        }
        let bytes = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        Ok(Saved { path, bytes })
    }
}

impl Drop for Session {
    /// If the app is closed mid-recording, still finalise the MKV properly.
    fn drop(&mut self) {
        if self.running {
            self.halt();
        }
    }
}
