//! Microphone + speaker (WASAPI loopback) capture, mixed to 48 kHz stereo
//! and streamed to FFmpeg through its stdin pipe (no network sockets at all).
//!
//! The mixer runs on the wall clock, so gaps (loopback delivers nothing while
//! the PC is silent) become silence and audio never drifts from the video.
//!
//! The mixer never writes to FFmpeg itself. FFmpeg 8 sometimes stops reading
//! the pipe for several seconds to keep its inputs in sync; a blocking write
//! would stall the mixer and the device buffers would overflow (lost audio).
//! Instead, mixed audio goes through an unbounded queue to a writer thread,
//! which feeds FFmpeg at whatever pace it reads.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use std::collections::VecDeque;
use std::io::Write;
use std::process::ChildStdin;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const RATE: u32 = 48_000;
const PREFILL_S: f64 = 0.12;
const PRIME_SAMPLES: usize = (RATE as usize / 25) * 2; // 40 ms stereo
const MAX_BACKLOG: usize = (RATE as usize / 4) * 2; // 250 ms
const KEEP_BACKLOG: usize = (RATE as usize / 12) * 2; // ~80 ms
/// Safety buffer per audio device, in samples (10 s stereo).
const DEVICE_CAP: usize = RATE as usize * 2 * 10;
/// Bytes per second of mixed audio (48 kHz, stereo, f32) - about 0.37 MB/s.
const BYTES_PER_SEC: usize = RATE as usize * 2 * 4;
/// Audio waiting for FFmpeg is dropped only beyond this (10 min, ~220 MB),
/// which only happens if FFmpeg has hung.
const MAX_PENDING: usize = BYTES_PER_SEC * 600;
/// If the mic gives only exact digital silence this long, Windows is blocking it.
const MIC_SILENT_WARN: Duration = Duration::from_secs(3);

pub struct Controls {
    pub mic_on: AtomicBool,
    pub spk_on: AtomicBool,
    /// Live subtitles take this audio (silence while off, to keep timing).
    pub subs_on: AtomicBool,
    pub stop: AtomicBool,
    pub mic_level: AtomicU32,
    pub spk_level: AtomicU32,
}

impl Controls {
    pub fn new(mic_on: bool, spk_on: bool) -> Self {
        Self {
            mic_on: AtomicBool::new(mic_on),
            spk_on: AtomicBool::new(spk_on),
            subs_on: AtomicBool::new(true),
            stop: AtomicBool::new(false),
            mic_level: AtomicU32::new(0),
            spk_level: AtomicU32::new(0),
        }
    }
    pub fn mic_level(&self) -> f32 {
        f32::from_bits(self.mic_level.load(Ordering::Relaxed))
    }
}

pub fn input_device_names() -> Vec<String> {
    let host = cpal::default_host();
    match host.input_devices() {
        Ok(it) => it.filter_map(|d| d.name().ok()).collect(),
        Err(_) => Vec::new(),
    }
}

type Queue = Arc<Mutex<VecDeque<f32>>>;

/// Appends 48 kHz stereo samples, keeping at most DEVICE_CAP.
fn push_queue(q: &Queue, data: &[f32]) {
    if let Ok(mut g) = q.lock() {
        g.extend(data.iter().copied());
        if g.len() > DEVICE_CAP {
            let extra = (g.len() - DEVICE_CAP) & !1;
            g.drain(..extra);
        }
    }
}

/// Sets the flag when dropped, so capture threads end with the mixer.
struct StopOnDrop(Arc<AtomicBool>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// Linear resampler to 48 kHz for devices running at other rates.
struct Resampler {
    step: f64,
    pos: f64,
    prev: [f32; 2],
    primed: bool,
}

impl Resampler {
    fn new(in_rate: u32) -> Self {
        Self {
            step: in_rate as f64 / RATE as f64,
            pos: 0.0,
            prev: [0.0; 2],
            primed: false,
        }
    }
    fn push(&mut self, f: [f32; 2], out: &mut Vec<f32>) {
        if (self.step - 1.0).abs() < 1e-9 {
            out.push(f[0]);
            out.push(f[1]);
            return;
        }
        if !self.primed {
            self.prev = f;
            self.primed = true;
            return;
        }
        while self.pos < 1.0 {
            let t = self.pos as f32;
            out.push(self.prev[0] + (f[0] - self.prev[0]) * t);
            out.push(self.prev[1] + (f[1] - self.prev[1]) * t);
            self.pos += self.step;
        }
        self.pos -= 1.0;
        self.prev = f;
    }
}

fn build_stream(device: &cpal::Device, loopback: bool, q: Queue) -> Result<cpal::Stream, String> {
    let cfg = if loopback {
        device.default_output_config()
    } else {
        device.default_input_config()
    }
    .map_err(|e| e.to_string())?;
    let channels = cfg.channels() as usize;
    let rate = cfg.sample_rate().0;
    let fmt = cfg.sample_format();
    let config: cpal::StreamConfig = cfg.into();
    match fmt {
        cpal::SampleFormat::F32 => make::<f32>(device, &config, channels, rate, q),
        cpal::SampleFormat::I16 => make::<i16>(device, &config, channels, rate, q),
        cpal::SampleFormat::U16 => make::<u16>(device, &config, channels, rate, q),
        cpal::SampleFormat::I32 => make::<i32>(device, &config, channels, rate, q),
        cpal::SampleFormat::F64 => make::<f64>(device, &config, channels, rate, q),
        other => Err(format!("unsupported sample format {other:?}")),
    }
}

fn make<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    rate: u32,
    q: Queue,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + Send + 'static,
    f32: FromSample<T>,
{
    let mut rs = Resampler::new(rate);
    let mut buf: Vec<f32> = Vec::with_capacity(8192);
    let stream = device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                buf.clear();
                for frame in data.chunks(channels.max(1)) {
                    if frame.len() < channels {
                        continue;
                    }
                    let l: f32 = frame[0].to_sample::<f32>();
                    let r: f32 = if channels > 1 { frame[1].to_sample::<f32>() } else { l };
                    let (l, r) = if channels >= 6 {
                        let c = frame[2].to_sample::<f32>() * 0.707;
                        (l + c, r + c)
                    } else {
                        (l, r)
                    };
                    rs.push([l, r], &mut buf);
                }
                push_queue(&q, &buf);
            },
            |e| eprintln!("audio stream error: {e}"),
            None,
        )
        .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

struct Source {
    q: Queue,
    primed: bool,
}

impl Source {
    fn new() -> Self {
        Self {
            q: Arc::new(Mutex::new(VecDeque::new())),
            primed: false,
        }
    }
    fn clear(&mut self) {
        if let Ok(mut g) = self.q.lock() {
            g.clear();
        }
        self.primed = false;
    }
    /// Take `frames` stereo frames; underruns are left for the caller to zero-fill.
    fn take(&mut self, frames: usize, out: &mut Vec<f32>) {
        out.clear();
        let Ok(mut g) = self.q.lock() else { return };
        if !self.primed {
            if g.len() >= PRIME_SAMPLES {
                self.primed = true;
            } else {
                return;
            }
        }
        let want = frames * 2;
        let have = g.len().min(want) & !1;
        out.extend(g.drain(..have));
        if have < want {
            self.primed = false;
        }
        if g.len() > MAX_BACKLOG {
            let extra = (g.len() - KEEP_BACKLOG) & !1;
            g.drain(..extra);
        }
    }
}

/// Automatic gain for the microphone plus "ducking" of system sound.
///
/// Built-in laptop mic arrays record speech at roughly -30..-40 dBFS, far
/// below typical system sound, so the voice gets buried. The gain lifts
/// speech towards VOICE_TARGET (up to +20 dB). In pauses the gain is held,
/// so background hiss is not pumped up. While the user speaks, system sound
/// is lowered by ~9 dB so the voice stays clear.
struct Voice {
    env: f32,
    gain: f32,
    duck: f32,
}

/// Target peak level of speech after the gain (about -10 dBFS).
const VOICE_TARGET: f32 = 0.32;
const MAX_GAIN: f32 = 10.0; // +20 dB
/// Below this raw level (~ -54 dBFS) the input is treated as background noise.
const NOISE_GATE: f32 = 0.002;
/// Boosted voice above this (~ -26 dBFS) counts as "user is speaking".
const SPEAKING: f32 = 0.05;
/// System-sound level while the user speaks (~ -9 dB).
const DUCK_LEVEL: f32 = 0.35;

impl Voice {
    fn new() -> Self {
        // Start with a moderate boost so the first words are not too quiet.
        Self {
            env: 0.0,
            gain: 3.0,
            duck: 1.0,
        }
    }

    /// `peak` = raw mic peak of this chunk of `frames`. Returns
    /// (gain at start, gain at end, duck at start, duck at end) so the
    /// caller can ramp smoothly across the chunk.
    fn update(&mut self, peak: f32, frames: usize, mic_on: bool) -> (f32, f32, f32, f32) {
        let dt = frames as f32 / RATE as f32;
        let follow = |tc: f32| 1.0 - (-dt / tc).exp();
        let (g0, d0) = (self.gain, self.duck);

        // Peak envelope: fast rise, 0.4 s fall.
        if peak > self.env {
            self.env += (peak - self.env) * follow(0.005);
        } else {
            self.env += (peak - self.env) * follow(0.4);
        }

        if self.env > NOISE_GATE {
            let want = (VOICE_TARGET / self.env).clamp(1.0, MAX_GAIN);
            // Turn down quickly on loud speech, turn up slowly.
            let tc = if want < self.gain { 0.02 } else { 1.5 };
            self.gain += (want - self.gain) * follow(tc);
        }

        let speaking = mic_on && self.env * self.gain > SPEAKING;
        let want_duck = if speaking { DUCK_LEVEL } else { 1.0 };
        let tc = if want_duck < self.duck { 0.06 } else { 0.6 };
        self.duck += (want_duck - self.duck) * follow(tc);

        (g0, self.gain, d0, self.duck)
    }
}

/// 48 kHz -> 16 kHz for speech recognition: averages each 3 samples (a
/// simple low-pass), which is plenty for voice.
#[derive(Default)]
struct Down3 {
    acc: f32,
    k: u8,
}

impl Down3 {
    fn push(&mut self, x: f32) -> Option<f32> {
        self.acc += x;
        self.k += 1;
        if self.k == 3 {
            let y = self.acc / 3.0;
            self.acc = 0.0;
            self.k = 0;
            Some(y)
        } else {
            None
        }
    }
}

/// One output sample. With SRec's speaker button off, PC sound is left out
/// completely (the subtitles use this same mix, so they skip it too).
fn mix(voice: f32, pc: f32, spk_on: bool, duck: f32) -> f32 {
    let pc = if spk_on { pc * duck } else { 0.0 };
    soft_clip(voice + pc)
}

fn soft_clip(x: f32) -> f32 {
    let a = x.abs();
    if a <= 0.9 {
        x
    } else {
        x.signum() * (0.9 + 0.1 * ((a - 0.9) / 0.1).tanh())
    }
}

pub struct AudioEngine {
    handle: Option<JoinHandle<()>>,
    writer: JoinHandle<()>,
    ctl: Arc<Controls>,
    pub warnings: Arc<Mutex<Vec<String>>>,
}

impl AudioEngine {
    /// Stops capture. Audio still queued keeps flowing to FFmpeg in the
    /// background; FFmpeg's stdin closes once all of it has been written.
    pub fn stop(&mut self) {
        self.ctl.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }

    /// True once every queued sample has reached FFmpeg (or FFmpeg is gone).
    pub fn writer_done(&self) -> bool {
        self.writer.is_finished()
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.stop();
    }
}

/// `sink` is FFmpeg's stdin. Closing it (on stop) ends the recording cleanly.
/// `subs` (optional) receives the same mixed audio as 16 kHz mono for live
/// subtitles, sample-aligned with the recording.
pub fn start(
    ctl: Arc<Controls>,
    mic_name: Option<String>,
    sink: ChildStdin,
    subs: Option<mpsc::Sender<Vec<f32>>>,
) -> Result<AudioEngine, String> {
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let pending = Arc::new(AtomicUsize::new(0));
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let p2 = pending.clone();
    let writer = std::thread::Builder::new()
        .name("audio-writer".into())
        .spawn(move || write_loop(sink, rx, p2))
        .map_err(|e| e.to_string())?;
    let (c2, w2) = (ctl.clone(), warnings.clone());
    let handle = std::thread::Builder::new()
        .name("audio-mixer".into())
        .spawn(move || run(tx, pending, c2, mic_name, w2, subs))
        .map_err(|e| e.to_string())?;
    Ok(AudioEngine {
        handle: Some(handle),
        writer,
        ctl,
        warnings,
    })
}

/// Feeds FFmpeg's stdin. Blocks whenever FFmpeg pauses reading, without
/// affecting the mixer. Ends (closing stdin) once the mixer has stopped and
/// the queue is empty.
fn write_loop(mut sink: ChildStdin, rx: mpsc::Receiver<Vec<u8>>, pending: Arc<AtomicUsize>) {
    for chunk in rx {
        let ok = sink.write_all(&chunk).is_ok();
        pending.fetch_sub(chunk.len(), Ordering::Relaxed);
        if !ok {
            // FFmpeg exited; dropping `rx` makes the mixer stop too.
            return;
        }
    }
    let _ = sink.flush();
}

fn run(
    tx: mpsc::Sender<Vec<u8>>,
    pending: Arc<AtomicUsize>,
    ctl: Arc<Controls>,
    mic_name: Option<String>,
    warnings: Arc<Mutex<Vec<String>>>,
    mut subs: Option<mpsc::Sender<Vec<f32>>>,
) {
    let warn = |m: String| {
        if let Ok(mut w) = warnings.lock() {
            w.push(m);
        }
    };
    let host = cpal::default_host();
    let mut mic = Source::new();
    let mut spk = Source::new();

    let mic_dev = match &mic_name {
        Some(n) => host
            .input_devices()
            .ok()
            .and_then(|mut it| it.find(|d| d.name().map(|x| &x == n).unwrap_or(false)))
            .or_else(|| host.default_input_device()),
        None => host.default_input_device(),
    };
    let _mic_stream = match mic_dev {
        Some(d) => match build_stream(&d, false, mic.q.clone()) {
            Ok(s) => Some(s),
            Err(e) => {
                warn(format!("Microphone unavailable: {e}"));
                None
            }
        },
        None => {
            warn("No microphone found".into());
            None
        }
    };
    // PC sound: process loopback records it at its real level, independent of
    // the speaker volume / mute. Older Windows: classic speaker loopback.
    let pc_stop = Arc::new(AtomicBool::new(false));
    let _pc_guard = StopOnDrop(pc_stop.clone());
    let q = spk.q.clone();
    let pc_thread = crate::loopback::start(move |data| push_queue(&q, data), pc_stop).ok();
    let _spk_stream = match (&pc_thread, host.default_output_device()) {
        (Some(_), _) => None,
        (None, Some(d)) => match build_stream(&d, true, spk.q.clone()) {
            Ok(s) => Some(s),
            Err(e) => {
                warn(format!("Speaker capture unavailable: {e}"));
                None
            }
        },
        (None, None) => None,
    };

    mic.clear();
    spk.clear();

    let start = Instant::now();
    let mut written: u64 = 0;
    let (mut m, mut s) = (Vec::new(), Vec::new());
    let mut mic_checked = _mic_stream.is_none();
    let mut overflow_warned = false;
    let mut voice = Voice::new();
    let mut down = Down3::default();

    while !ctl.stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(10));
        let el = start.elapsed().as_secs_f64() - PREFILL_S;
        if el <= 0.0 {
            continue;
        }
        let target = (el * RATE as f64) as u64;
        if target <= written {
            continue;
        }
        let n = (target - written) as usize;
        mic.take(n, &mut m);
        spk.take(n, &mut s);
        let mic_on = ctl.mic_on.load(Ordering::Relaxed);
        let spk_on = ctl.spk_on.load(Ordering::Relaxed);
        let subs_on = ctl.subs_on.load(Ordering::Relaxed);
        let mut bytes: Vec<u8> = Vec::with_capacity(n * 8);
        // Voice is mono: built-in mic arrays give slightly different left /
        // right signals; averaging them keeps the voice centred.
        let (mut mp, mut sp) = (0f32, 0f32);
        for f in 0..n {
            let a = (m.get(2 * f).copied().unwrap_or(0.0) + m.get(2 * f + 1).copied().unwrap_or(0.0)) * 0.5;
            mp = mp.max(a.abs());
        }
        let (g0, g1, d0, d1) = voice.update(mp, n, mic_on);
        let mut out_peak = 0f32;
        let mut sub_buf: Vec<f32> = Vec::with_capacity(if subs.is_some() { n / 3 + 1 } else { 0 });
        for f in 0..n {
            let t = (f as f32 + 1.0) / n as f32;
            let g = g0 + (g1 - g0) * t;
            let duck = d0 + (d1 - d0) * t;
            let raw = (m.get(2 * f).copied().unwrap_or(0.0) + m.get(2 * f + 1).copied().unwrap_or(0.0)) * 0.5;
            let a = if mic_on { raw * g } else { 0.0 };
            out_peak = out_peak.max(a.abs());
            let mut mono = 0.0;
            for ch in 0..2 {
                let b = s.get(2 * f + ch).copied().unwrap_or(0.0);
                sp = sp.max(b.abs());
                let x = mix(a, b, spk_on, duck);
                mono += x * 0.5;
                bytes.extend_from_slice(&x.to_le_bytes());
            }
            if subs.is_some() {
                let mono = if subs_on { mono } else { 0.0 };
                if let Some(y) = down.push(mono) {
                    sub_buf.push(y);
                }
            }
        }
        if let Some(tx) = &subs {
            if tx.send(sub_buf).is_err() {
                subs = None; // subtitle thread gone; recording carries on
            }
        }
        // The meter shows the boosted voice, i.e. what ends up in the file.
        ctl.mic_level.store(
            (if mic_on { out_peak.min(1.0) } else { 0.0f32 }).to_bits(),
            Ordering::Relaxed,
        );
        ctl.spk_level.store(sp.to_bits(), Ordering::Relaxed);
        written = target;

        // Windows hands apps pure digital silence when mic access is off.
        if !mic_checked {
            if mp > 0.0 {
                mic_checked = true;
            } else if mic_on && el > MIC_SILENT_WARN.as_secs_f64() {
                mic_checked = true;
                warn(
                    "Microphone is silent - check Settings > Privacy & security > Microphone > \
                     Let desktop apps access your microphone"
                        .into(),
                );
            }
        }

        if pending.load(Ordering::Relaxed) + bytes.len() > MAX_PENDING {
            // FFmpeg has not read audio for minutes, so it has hung. Drop
            // rather than use unlimited memory.
            if !overflow_warned {
                overflow_warned = true;
                warn("FFmpeg stopped reading audio - some audio was lost".into());
            }
            continue;
        }
        let len = bytes.len();
        pending.fetch_add(len, Ordering::Relaxed);
        if tx.send(bytes).is_err() {
            // The writer ended because FFmpeg exited.
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run `secs` of 10 ms chunks with the given raw mic peak.
    fn feed(v: &mut Voice, peak: f32, secs: f32) {
        for _ in 0..(secs * 100.0) as usize {
            v.update(peak, 480, true);
        }
    }

    #[test]
    fn speaker_off_leaves_out_pc_sound() {
        assert_eq!(mix(0.0, 0.8, false, 1.0), 0.0);
        assert_eq!(mix(0.2, 0.8, false, 1.0), 0.2);
        assert!((mix(0.0, 0.5, true, 1.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn quiet_laptop_speech_is_boosted() {
        let mut v = Voice::new();
        feed(&mut v, 0.01, 4.0); // speech at -40 dBFS
        let out = 0.01 * v.gain;
        assert!(out > 0.08, "boosted voice only {out}");
        assert!(v.duck < 0.5, "system sound not lowered while speaking");
    }

    #[test]
    fn loud_speech_is_not_boosted() {
        let mut v = Voice::new();
        feed(&mut v, 0.5, 1.0);
        assert!((v.gain - 1.0).abs() < 0.05, "gain {}", v.gain);
    }

    #[test]
    fn background_noise_is_not_pumped_up_and_no_ducking() {
        let mut v = Voice::new();
        feed(&mut v, 0.0005, 5.0); // -66 dBFS hiss
        assert!(v.gain <= 3.0 + 1e-3);
        assert!(v.duck > 0.95);
    }

    #[test]
    fn ducking_releases_after_speech() {
        let mut v = Voice::new();
        feed(&mut v, 0.02, 3.0);
        feed(&mut v, 0.0, 3.0);
        assert!(v.duck > 0.95, "duck {}", v.duck);
    }
}
