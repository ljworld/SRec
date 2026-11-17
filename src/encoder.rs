//! FFmpeg discovery, AV1 encoder selection and argument building.

use crate::gpu::{Adapter, Display, Vendor};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Enc {
    Nvenc,
    Qsv,
    Amf,
    Svt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capture {
    /// Desktop Duplication, frames stay on the GPU (zero copy).
    DdaDirect,
    /// Desktop Duplication mapped into Intel QSV memory.
    DdaQsv,
    /// Desktop Duplication, frames copied to system memory.
    DdaDownload,
    /// Legacy GDI capture (works over Remote Desktop / old drivers).
    Gdi,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pipeline {
    pub enc: Enc,
    pub cap: Capture,
}

impl Pipeline {
    pub fn short(&self) -> &'static str {
        match self.enc {
            Enc::Nvenc => "NVENC AV1",
            Enc::Qsv => "Quick Sync AV1",
            Enc::Amf => "AMD AV1",
            Enc::Svt => "SVT-AV1 (CPU)",
        }
    }
    pub fn label(&self) -> String {
        let cap = match self.cap {
            Capture::DdaDirect | Capture::DdaQsv => "zero-copy GPU capture",
            Capture::DdaDownload => "GPU capture",
            Capture::Gdi => "GDI capture",
        };
        format!("{} - {}", self.short(), cap)
    }
    pub fn key(&self) -> String {
        format!("{:?}:{:?}", self.enc, self.cap)
    }
    pub fn from_key(s: &str) -> Option<Self> {
        let (e, c) = s.split_once(':')?;
        let enc = match e {
            "Nvenc" => Enc::Nvenc,
            "Qsv" => Enc::Qsv,
            "Amf" => Enc::Amf,
            "Svt" => Enc::Svt,
            _ => return None,
        };
        let cap = match c {
            "DdaDirect" => Capture::DdaDirect,
            "DdaQsv" => Capture::DdaQsv,
            "DdaDownload" => Capture::DdaDownload,
            "Gdi" => Capture::Gdi,
            _ => return None,
        };
        Some(Self { enc, cap })
    }
}

/// Hourly size targets (MiB per hour of recording, video + audio).
pub const BUDGETS: [u32; 5] = [400, 500, 700, 1000, 1500];
pub const DEFAULT_BUDGET: u32 = 700;

pub fn budget_label(mb: u32) -> String {
    let what = match mb {
        0..=450 => "meetings / slides",
        451..=600 => "office work",
        601..=800 => "1080p tutorials (default)",
        801..=1200 => "video on screen",
        _ => "fast motion / games",
    };
    if mb >= 1000 {
        format!("max {:.1} GB per hour - {what}", mb as f64 / 1024.0)
    } else {
        format!("max {mb} MB per hour - {what}")
    }
}

pub fn next_budget(mb: u32) -> u32 {
    let i = BUDGETS.iter().position(|&b| b == mb).unwrap_or(1);
    BUDGETS[(i + 1) % BUDGETS.len()]
}

/// What to capture: a display, optionally cropped to a region,
/// optionally scaled down so the longest side fits 1920x1080.
#[derive(Clone, Debug)]
pub struct VideoParams {
    pub fps: u32,
    /// Hard ceiling for the whole file, MiB per hour.
    pub mb_per_hour: u32,
    pub output_idx: u32,
    pub disp_x: i32,
    pub disp_y: i32,
    pub disp_w: i32,
    pub disp_h: i32,
    /// Capture region in absolute desktop pixels (always even sized).
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// Encoded size (after optional downscale), even.
    pub ow: i32,
    pub oh: i32,
}

impl VideoParams {
    /// `region` = absolute x, y, w, h. It is clipped to the display and made even.
    pub fn new(d: &Display, region: Option<[i32; 4]>, fps: u32, mb_per_hour: u32, max_1080: bool) -> Self {
        let (mut x, mut y, mut w, mut h) = (d.x, d.y, d.w, d.h);
        if let Some([rx, ry, rw, rh]) = region {
            let l = rx.max(d.x);
            let t = ry.max(d.y);
            let r = (rx + rw).min(d.x + d.w);
            let b = (ry + rh).min(d.y + d.h);
            if r - l >= 64 && b - t >= 64 {
                x = l;
                y = t;
                w = r - l;
                h = b - t;
            }
        }
        w -= w % 2;
        h -= h % 2;
        let (mut ow, mut oh) = (w, h);
        if max_1080 {
            // Fit inside 1920x1080 (or 1080x1920 for portrait), keep aspect.
            let (bw, bh) = if w >= h { (1920.0, 1080.0) } else { (1080.0, 1920.0) };
            let f = (bw / w as f64).min(bh / h as f64);
            if f < 1.0 {
                ow = ((w as f64 * f).round() as i32) & !1;
                oh = ((h as f64 * f).round() as i32) & !1;
            }
        }
        Self {
            fps,
            mb_per_hour,
            output_idx: d.index,
            disp_x: d.x,
            disp_y: d.y,
            disp_w: d.w,
            disp_h: d.h,
            x,
            y,
            w,
            h,
            ow,
            oh,
        }
    }

    fn is_full_display(&self) -> bool {
        self.x == self.disp_x && self.y == self.disp_y && self.w == self.disp_w && self.h == self.disp_h
    }

    pub fn scaled(&self) -> bool {
        self.ow != self.w || self.oh != self.h
    }

    pub fn audio_kbps(&self) -> u32 {
        match self.mb_per_hour {
            0..=450 => 64,
            451..=800 => 96,
            _ => 128,
        }
    }

    /// Video ceiling (kbit/s) so that video + audio + container overhead
    /// can never exceed `mb_per_hour`.
    pub fn video_cap_kbps(&self) -> u32 {
        let total = self.mb_per_hour as f64 * 1_048_576.0 * 8.0 / 3600.0 / 1000.0;
        (((total - self.audio_kbps() as f64) * 0.97).max(300.0)) as u32
    }

    /// Quality target. Lower = better. The encoder only spends what the
    /// picture needs, so calm screens land far below the ceiling.
    fn quality_level(&self) -> u32 {
        match self.mb_per_hour {
            0..=450 => 35,
            451..=600 => 33,
            601..=800 => 31,
            801..=1200 => 29,
            _ => 27,
        }
    }
}

/// Look for ffmpeg.exe next to the app, on PATH, or in the winget install folders.
pub fn find_ffmpeg() -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for p in [
                dir.join("ffmpeg.exe"),
                dir.join("ffmpeg").join("bin").join("ffmpeg.exe"),
            ] {
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for d in std::env::split_paths(&path) {
            let p = d.join("ffmpeg.exe");
            if p.is_file() {
                return Some(p);
            }
        }
    }
    if let Some(la) = std::env::var_os("LOCALAPPDATA") {
        let winget = PathBuf::from(la).join("Microsoft").join("WinGet");
        let link = winget.join("Links").join("ffmpeg.exe");
        if link.is_file() {
            return Some(link);
        }
        if let Ok(rd) = std::fs::read_dir(winget.join("Packages")) {
            for e in rd.flatten() {
                if !e.file_name().to_string_lossy().starts_with("Gyan.FFmpeg") {
                    continue;
                }
                if let Ok(inner) = std::fs::read_dir(e.path()) {
                    for sub in inner.flatten() {
                        let p = sub.path().join("bin").join("ffmpeg.exe");
                        if p.is_file() {
                            return Some(p);
                        }
                    }
                }
            }
        }
    }
    None
}

/// Candidate pipelines ordered best-first for the detected graphics cards.
pub fn candidates(adapters: &[Adapter]) -> Vec<Pipeline> {
    let has = |v: Vendor| adapters.iter().any(|a| a.vendor == v);
    let unknown = adapters.is_empty();
    let mut hw = Vec::new();
    if has(Vendor::Nvidia) || unknown {
        hw.push(Enc::Nvenc);
    }
    if has(Vendor::Amd) || unknown {
        hw.push(Enc::Amf);
    }
    if has(Vendor::Intel) || unknown {
        hw.push(Enc::Qsv);
    }
    let mut out = Vec::new();
    for &e in &hw {
        let direct = if e == Enc::Qsv {
            Capture::DdaQsv
        } else {
            Capture::DdaDirect
        };
        out.push(Pipeline { enc: e, cap: direct });
        out.push(Pipeline {
            enc: e,
            cap: Capture::DdaDownload,
        });
    }
    out.push(Pipeline {
        enc: Enc::Svt,
        cap: Capture::DdaDownload,
    });
    for &e in &hw {
        out.push(Pipeline {
            enc: e,
            cap: Capture::Gdi,
        });
    }
    out.push(Pipeline {
        enc: Enc::Svt,
        cap: Capture::Gdi,
    });
    out
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

/// Input arguments (and optional -vf filter) for the capture method.
/// When the source is larger than 1080p the frames are scaled down first
/// (Lanczos on CPU, or on the GPU for Quick Sync).
pub fn video_input(p: Pipeline, v: &VideoParams) -> (Vec<String>, Option<String>) {
    let mut dda = format!("ddagrab=output_idx={}:framerate={}:draw_mouse=1", v.output_idx, v.fps);
    if !v.is_full_display() {
        dda.push_str(&format!(
            ":offset_x={}:offset_y={}:video_size={}x{}",
            v.x - v.disp_x,
            v.y - v.disp_y,
            v.w,
            v.h
        ));
    }
    let pixfmt = if p.enc == Enc::Svt { "yuv420p" } else { "nv12" };
    let sw_scale = if v.scaled() {
        format!("scale={}:{}:flags=lanczos,", v.ow, v.oh)
    } else {
        String::new()
    };
    let lavfi = |graph: String| {
        let mut a = s(&["-thread_queue_size", "512", "-f", "lavfi", "-i"]);
        a.push(graph);
        a
    };
    let download = format!("{dda},hwdownload,format=bgra,{sw_scale}format={pixfmt}");
    match p.cap {
        // Zero-copy cannot resize on every GPU, so scale through system memory.
        Capture::DdaDirect if v.scaled() => (lavfi(download), None),
        Capture::DdaDirect => (lavfi(dda), None),
        Capture::DdaQsv => {
            let sz = if v.scaled() {
                format!("w={}:h={}:", v.ow, v.oh)
            } else {
                String::new()
            };
            (
                lavfi(format!(
                    "{dda},hwmap=derive_device=qsv,format=qsv,scale_qsv={sz}format=nv12"
                )),
                None,
            )
        }
        Capture::DdaDownload => (lavfi(download), None),
        Capture::Gdi => {
            let mut a = s(&["-thread_queue_size", "512", "-f", "gdigrab", "-framerate"]);
            a.push(v.fps.to_string());
            a.extend(s(&["-draw_mouse", "1", "-offset_x"]));
            a.push(v.x.to_string());
            a.push("-offset_y".into());
            a.push(v.y.to_string());
            a.push("-video_size".into());
            a.push(format!("{}x{}", v.w, v.h));
            a.extend(s(&["-i", "desktop"]));
            (a, Some(format!("{sw_scale}format={pixfmt}")))
        }
    }
}

/// Quality-first VBR with a hard ceiling ("capped constant quality"):
/// * the encoder aims at a visual quality level, so a still or slow screen
///   uses only a fraction of the budget;
/// * `maxrate` = the hourly budget, with a 20 s buffer to absorb bursts
///   (scrolling, scene changes) - the file can never exceed the budget;
/// * 10 s GOP: unchanged frames cost a few bytes each.
pub fn video_encoder(enc: Enc, v: &VideoParams) -> Vec<String> {
    let cap = v.video_cap_kbps();
    let q = v.quality_level();
    let g = (v.fps * 10).to_string();
    let k = |x: u32| format!("{x}k");
    let ceiling = |a: &mut Vec<String>, avg: Option<u32>, max: u32, buf_s: u32| {
        if let Some(avg) = avg {
            a.push("-b:v".into());
            a.push(k(avg));
        }
        a.extend([
            "-maxrate".into(),
            k(max),
            "-bufsize".into(),
            k(max * buf_s),
            "-g".into(),
            g.clone(),
        ]);
    };
    match enc {
        Enc::Nvenc => {
            // NVENC: true constant-quality VBR (-cq) under the ceiling.
            let mut a = s(&["-c:v", "av1_nvenc", "-preset", "p6", "-tune", "hq", "-rc", "vbr", "-cq"]);
            a.push(q.to_string());
            a.extend(s(&["-b:v", "0", "-spatial-aq", "1"]));
            ceiling(&mut a, None, cap, 20);
            a
        }
        Enc::Qsv => {
            // Quick Sync: VBR, average 55 % of the ceiling; calm screens undershoot.
            let mut a = s(&["-c:v", "av1_qsv", "-preset", "slow"]);
            ceiling(&mut a, Some(cap * 55 / 100), cap, 10);
            a
        }
        Enc::Amf => {
            let mut a = s(&[
                "-c:v",
                "av1_amf",
                "-usage",
                "transcoding",
                "-quality",
                "quality",
                "-rc",
                "vbr_peak",
            ]);
            ceiling(&mut a, Some(cap * 55 / 100), cap, 10);
            a
        }
        Enc::Svt => {
            // SVT-AV1: capped CRF. Screen-content tools (palette / IntraBC) are auto-enabled.
            let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
            let px = v.ow as i64 * v.oh as i64;
            let mut preset: u32 = if cores >= 16 {
                8
            } else if cores >= 8 {
                10
            } else {
                12
            };
            if px > 1920 * 1080 || v.fps > 30 {
                preset = (preset + 1).min(12);
            }
            let mut a = s(&["-c:v", "libsvtav1", "-preset"]);
            a.push(preset.to_string());
            a.push("-crf".into());
            a.push((q + 4).to_string());
            a.extend(s(&["-svtav1-params", "tune=0"]));
            // SVT's VBV overshoots ~20 % on extreme motion, so aim it lower.
            ceiling(&mut a, None, cap * 78 / 100, 10);
            a
        }
    }
}

/// Opus: 64-128 kbit/s stereo is transparent for voice + system sound.
pub fn audio_encoder(v: &VideoParams) -> Vec<String> {
    let mut a = s(&["-c:a", "libopus", "-b:a"]);
    a.push(format!("{}k", v.audio_kbps()));
    a.extend(s(&["-vbr", "on", "-application", "audio"]));
    a
}

/// Encode a few real captured frames to check the pipeline works on this PC.
pub fn test(ffmpeg: &PathBuf, p: Pipeline, v: &VideoParams) -> bool {
    let (input, vf) = video_input(p, v);
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-hide_banner", "-loglevel", "error", "-nostdin"])
        .args(&input);
    if let Some(f) = vf {
        cmd.args(["-vf", f.as_str()]);
    }
    cmd.args(["-frames:v", "10"])
        .args(video_encoder(p.enc, v))
        .args(["-f", "null", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW);
    let Ok(mut child) = cmd.spawn() else { return false };
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(st)) => return st.success(),
            Ok(None) => {
                if start.elapsed() > Duration::from_secs(20) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return false;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return false,
        }
    }
}
