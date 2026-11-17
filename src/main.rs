//! SRec - compact AV1 screen recorder with a Dynamic-Island style UI.
#![windows_subsystem = "windows"]

mod audio;
mod config;
mod encoder;
mod gpu;
mod loopback;
mod platform;
mod recorder;
mod select;
mod subtitles;

use config::Config;
use eframe::egui::{
    self, pos2, vec2, Align2, Color32, FontId, Painter, Pos2, Rect, Rounding, Sense, Shape, Stroke, Vec2,
    ViewportCommand,
};
use encoder::{Pipeline, VideoParams};
use select::Selection;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

const TITLE: &str = "SRec";
const ROW_H: f32 = 44.0;
const W_SMALL: f32 = 360.0;
const W_BIG: f32 = 600.0;
const SET_ROW: f32 = 26.0;

const RED: Color32 = Color32::from_rgb(255, 69, 58);
const GREEN: Color32 = Color32::from_rgb(48, 209, 88);
const AMBER: Color32 = Color32::from_rgb(255, 179, 64);
const BLUE: Color32 = Color32::from_rgb(10, 132, 255);
const TXT: Color32 = Color32::from_rgb(240, 240, 245);
const DIM: Color32 = Color32::from_rgb(150, 150, 160);

fn main() -> Result<(), eframe::Error> {
    let viewport = egui::ViewportBuilder::default()
        .with_title(TITLE)
        .with_inner_size([W_SMALL, ROW_H])
        .with_position([600.0, 10.0])
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_window_level(egui::WindowLevel::AlwaysOnTop)
        .with_taskbar(false);
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(TITLE, options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}

// ---------------------------------------------------------------------------

struct ProbeResult {
    ffmpeg: Option<PathBuf>,
    adapters: Vec<gpu::Adapter>,
    displays: Vec<gpu::Display>,
    mics: Vec<String>,
    pipeline: Option<Pipeline>,
    cache_key: String,
}

enum Phase {
    Probing(mpsc::Receiver<ProbeResult>),
    MissingFfmpeg,
    Failed(String),
    Ready,
    Recording(Box<recorder::Session>),
    Saving(mpsc::Receiver<Result<recorder::Saved, String>>),
}

#[derive(Clone, Debug)]
enum Target {
    Display,
    Window { hwnd: isize, title: String, rect: [i32; 4] },
    Area([i32; 4]),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Act {
    None,
    Retest,
    Licenses,
    Budget,
    Resolution,
    Fps,
    Display,
    Mic,
    Target,
    Folder,
    Subtitles,
}

struct App {
    cfg: Config,
    phase: Phase,
    ffmpeg: Option<PathBuf>,
    pipeline: Option<Pipeline>,
    adapters: Vec<gpu::Adapter>,
    displays: Vec<gpu::Display>,
    mics: Vec<String>,
    target: Target,
    select_rx: Option<mpsc::Receiver<Selection>>,
    settings_open: bool,
    last_hover: Instant,
    positioned: bool,
    excluded_tries: u32,
    cur_size: Vec2,
    hk_record: Arc<AtomicBool>,
    hk_select: Arc<AtomicBool>,
    toast: Option<(String, Instant)>,
    last_saved: Option<PathBuf>,
    whisper: Option<subtitles::Whisper>,
    /// Live subtitle jobs still finishing (one per recording).
    live_subs: Vec<mpsc::Receiver<subtitles::Event>>,
    /// Subtitle result that arrived before the video finished saving.
    subs_note: Option<String>,
    ctx: egui::Context,
}

fn spawn_probe(cfg: &Config, force: bool, ctx: egui::Context) -> mpsc::Receiver<ProbeResult> {
    let (tx, rx) = mpsc::channel();
    let cached_key = cfg.cache_key.clone();
    let cached_pipe = cfg.cache_pipeline.clone();
    let fps = cfg.fps;
    let budget = cfg.mb_per_hour;
    let max_1080 = cfg.max_1080;
    let disp = cfg.display;
    std::thread::spawn(move || {
        let ffmpeg = encoder::find_ffmpeg();
        let (adapters, displays) = gpu::detect();
        let mics = audio::input_device_names();
        let mut res = ProbeResult {
            ffmpeg: ffmpeg.clone(),
            adapters,
            displays,
            mics,
            pipeline: None,
            cache_key: String::new(),
        };
        if let Some(ff) = ffmpeg {
            let names: Vec<&str> = res.adapters.iter().map(|a| a.name.as_str()).collect();
            let key = format!("{}|{}", ff.display(), names.join(","));
            let d = res
                .displays
                .get(disp)
                .or(res.displays.first())
                .cloned()
                .unwrap_or_default();
            let v = VideoParams::new(&d, None, fps, budget, max_1080);
            let mut chosen = None;
            if !force && key == cached_key {
                if let Some(p) = Pipeline::from_key(&cached_pipe) {
                    if encoder::test(&ff, p, &v) {
                        chosen = Some(p);
                    }
                }
            }
            if chosen.is_none() {
                for p in encoder::candidates(&res.adapters) {
                    if encoder::test(&ff, p, &v) {
                        chosen = Some(p);
                        break;
                    }
                }
            }
            res.pipeline = chosen;
            res.cache_key = key;
        }
        let _ = tx.send(res);
        ctx.request_repaint();
    });
    rx
}

fn fmt_dur(d: Duration) -> String {
    let s = d.as_secs();
    format!("{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}

fn fmt_size(b: u64) -> String {
    let mb = b as f64 / 1_048_576.0;
    if mb >= 1024.0 {
        format!("{:.2} GB", mb / 1024.0)
    } else {
        format!("{:.1} MB", mb)
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let t: String = s.chars().take(n.saturating_sub(3)).collect();
        format!("{t}...")
    }
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let cfg = Config::load();
        let hk_record = Arc::new(AtomicBool::new(false));
        let hk_select = Arc::new(AtomicBool::new(false));
        platform::spawn_hotkeys(hk_record.clone(), hk_select.clone(), cc.egui_ctx.clone());
        let rx = spawn_probe(&cfg, false, cc.egui_ctx.clone());
        Self {
            cfg,
            phase: Phase::Probing(rx),
            ffmpeg: None,
            pipeline: None,
            adapters: Vec::new(),
            displays: Vec::new(),
            mics: Vec::new(),
            target: Target::Display,
            select_rx: None,
            settings_open: false,
            last_hover: Instant::now(),
            positioned: false,
            excluded_tries: 0,
            cur_size: vec2(W_SMALL, ROW_H),
            hk_record,
            hk_select,
            toast: None,
            last_saved: None,
            whisper: subtitles::find(),
            live_subs: Vec::new(),
            subs_note: None,
            ctx: cc.egui_ctx.clone(),
        }
    }

    fn subtitles_busy(&self) -> bool {
        !self.live_subs.is_empty()
    }

    fn toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    fn is_busy(&self) -> bool {
        matches!(self.phase, Phase::Recording(_) | Phase::Saving(_))
    }

    // ---- state polling ------------------------------------------------------

    fn poll(&mut self) {
        let mut next: Option<Phase> = None;
        let mut died: Option<String> = None;
        let mut new_toast: Option<String> = None;
        match &mut self.phase {
            Phase::Probing(rx) => {
                if let Ok(r) = rx.try_recv() {
                    self.ffmpeg = r.ffmpeg;
                    self.adapters = r.adapters;
                    self.displays = r.displays;
                    self.mics = r.mics;
                    self.pipeline = r.pipeline;
                    if self.cfg.display >= self.displays.len() {
                        self.cfg.display = 0;
                    }
                    next = Some(if self.ffmpeg.is_none() {
                        Phase::MissingFfmpeg
                    } else if let Some(p) = self.pipeline {
                        self.cfg.cache_key = r.cache_key;
                        self.cfg.cache_pipeline = p.key();
                        self.cfg.save();
                        Phase::Ready
                    } else {
                        Phase::Failed(
                            "No working AV1 encoder. Update the graphics driver, or use a full FFmpeg build.".into(),
                        )
                    });
                }
            }
            Phase::Recording(s) => {
                if let Some(e) = s.check_exited() {
                    died = Some(e);
                }
                if let Some(w) = s.take_warnings().pop() {
                    new_toast = Some(w);
                }
            }
            Phase::Saving(rx) => {
                if let Ok(r) = rx.try_recv() {
                    match r {
                        Ok(saved) => {
                            let mut t = format!("Saved {}", fmt_size(saved.bytes));
                            if !self.live_subs.is_empty() {
                                t.push_str("  ·  finishing subtitles");
                            } else if let Some(n) = self.subs_note.take() {
                                t.push_str(&format!("  ·  {n}"));
                            }
                            t.push_str("  -  click to show");
                            new_toast = Some(t);
                            self.last_saved = Some(saved.path);
                        }
                        Err(e) => new_toast = Some(e),
                    }
                    next = Some(Phase::Ready);
                }
            }
            _ => {}
        }
        if let Some(n) = next {
            self.phase = n;
        }
        if let Some(t) = new_toast {
            self.toast(t);
        }
        let mut events = Vec::new();
        self.live_subs.retain(|rx| match rx.try_recv() {
            Ok(ev) => {
                events.push(ev);
                false
            }
            Err(mpsc::TryRecvError::Empty) => true,
            Err(mpsc::TryRecvError::Disconnected) => false,
        });
        for ev in events {
            let (note, msg) = match ev {
                subtitles::Event::Done { lines } => (
                    format!("subtitles: {lines} lines"),
                    format!("Subtitles saved ({lines} lines)  -  click to show"),
                ),
                subtitles::Event::NoSpeech => (
                    "no speech for subtitles".to_string(),
                    "No speech found - no subtitles written".to_string(),
                ),
                subtitles::Event::Failed(e) => ("subtitles failed".to_string(), e),
            };
            if matches!(self.phase, Phase::Saving(_)) {
                // Shown together with "Saved ..." in a moment.
                self.subs_note = Some(note);
            } else {
                self.toast(msg);
            }
        }
        if let Some(e) = died {
            self.stop_recording();
            self.toast(e);
        }

        if let Some(rx) = &self.select_rx {
            if let Ok(sel) = rx.try_recv() {
                self.select_rx = None;
                self.apply_selection(sel);
            }
        }
    }

    fn apply_selection(&mut self, sel: Selection) {
        match sel {
            Selection::Cancelled => {}
            Selection::FullDisplay { x, y } => {
                if let Some(i) = self.displays.iter().position(|d| d.contains(x, y)) {
                    self.cfg.display = i;
                    self.cfg.save();
                }
                self.target = Target::Display;
                self.toast(format!("Capture: display {}", self.cfg.display + 1));
            }
            Selection::Window { hwnd, title, rect } => {
                self.toast(format!("Capture: {}", truncate(&title, 30)));
                self.target = Target::Window { hwnd, title, rect };
            }
            Selection::Area(r) => {
                self.toast(format!("Capture: area {} x {}", r[2], r[3]));
                self.target = Target::Area(r);
            }
        }
    }

    fn start_selection(&mut self) {
        if self.is_busy() || self.select_rx.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(select::pick());
        });
        self.select_rx = Some(rx);
    }

    // ---- recording ------------------------------------------------------------

    fn display_for(&self, region: Option<[i32; 4]>) -> gpu::Display {
        if let Some([x, y, w, h]) = region {
            let (cx, cy) = (x + w / 2, y + h / 2);
            if let Some(d) = self.displays.iter().find(|d| d.contains(cx, cy)) {
                return d.clone();
            }
        }
        self.displays
            .get(self.cfg.display)
            .or(self.displays.first())
            .cloned()
            .unwrap_or_default()
    }

    fn video_params(&self) -> VideoParams {
        let region = match &self.target {
            Target::Display => None,
            Target::Window { hwnd, rect, .. } => Some(select::window_rect(*hwnd).unwrap_or(*rect)),
            Target::Area(r) => Some(*r),
        };
        let d = self.display_for(region);
        VideoParams::new(&d, region, self.cfg.fps, self.cfg.mb_per_hour, self.cfg.max_1080)
    }

    fn toggle_record(&mut self) {
        match self.phase {
            Phase::Ready => self.start_recording(),
            Phase::Recording(_) => self.stop_recording(),
            _ => {}
        }
    }

    fn start_recording(&mut self) {
        let (Some(ff), Some(p)) = (self.ffmpeg.clone(), self.pipeline) else {
            return;
        };
        if let Target::Window { hwnd, .. } = &self.target {
            select::focus(*hwnd);
        }
        let v = self.video_params();
        let mic = if self.cfg.mic_device.is_empty() {
            None
        } else {
            Some(self.cfg.mic_device.clone())
        };
        let audio_in = recorder::AudioSetup {
            mic,
            mic_on: self.cfg.mic_enabled,
            spk_on: self.cfg.spk_enabled,
            subs: self
                .whisper
                .clone()
                .filter(|_| self.cfg.subtitles)
                .map(|w| (w, self.ctx.clone())),
        };
        match recorder::start(&ff, p, &v, &self.cfg.out_dir, audio_in) {
            Ok(mut s) => {
                if let Some(rx) = s.subs.take() {
                    self.live_subs.push(rx);
                }
                self.subs_note = None;
                self.toast = None;
                self.phase = Phase::Recording(Box::new(s));
            }
            Err(e) => self.toast(e),
        }
    }

    fn stop_recording(&mut self) {
        if !matches!(self.phase, Phase::Recording(_)) {
            return;
        }
        if let Phase::Recording(s) = std::mem::replace(&mut self.phase, Phase::Ready) {
            let ff = self.ffmpeg.clone().unwrap_or_default();
            self.phase = Phase::Saving((*s).stop(ff));
        }
    }

    // ---- text -----------------------------------------------------------------

    fn target_short(&self) -> String {
        match &self.target {
            Target::Display => {
                if self.displays.len() > 1 {
                    format!("Display {}", self.cfg.display + 1)
                } else {
                    "Full screen".into()
                }
            }
            Target::Window { title, .. } => truncate(title, 18),
            Target::Area(r) => format!("Area {}x{}", r[2], r[3]),
        }
    }

    fn status_text(&self) -> (String, Color32) {
        if let Some((m, at)) = &self.toast {
            if at.elapsed() < Duration::from_secs(5) {
                return (m.clone(), TXT);
            }
        }
        match &self.phase {
            Phase::Probing(_) => ("Checking graphics card...".into(), TXT),
            Phase::MissingFfmpeg => ("ffmpeg.exe missing - please reinstall SRec".into(), AMBER),
            Phase::Failed(_) => ("No AV1 encoder - click for details".into(), AMBER),
            Phase::Saving(_) => ("Saving MP4...".into(), TXT),
            Phase::Recording(s) => {
                let el = s.started.elapsed();
                let size = s.size();
                let mut t = format!("{}   {}", fmt_dur(el), fmt_size(size));
                // Live projection of the hourly size once the rate has settled.
                if el.as_secs() >= 20 {
                    let per_hour = size as f64 / el.as_secs_f64() * 3600.0;
                    t.push_str(&format!("   ~{}/h", fmt_size(per_hour as u64)));
                }
                if !self.cfg.mic_enabled {
                    t.push_str("   mic off");
                }
                (t, TXT)
            }
            Phase::Ready if self.subtitles_busy() => ("Ready  ·  writing subtitles...".into(), TXT),
            Phase::Ready => {
                let enc = self.pipeline.map(|p| p.short()).unwrap_or("AV1");
                (
                    format!("{}  ·  {}  ·  {} MB/h", enc, self.target_short(), self.cfg.mb_per_hour),
                    TXT,
                )
            }
        }
    }

    fn settings_rows(&self) -> Vec<(&'static str, String, Act)> {
        let busy = self.is_busy();
        let lock = |a: Act| if busy { Act::None } else { a };
        let mut v = Vec::new();
        if let Phase::Failed(e) = &self.phase {
            v.push(("Problem", e.clone(), Act::None));
        }
        let gpus = if self.adapters.is_empty() {
            "Not detected".to_string()
        } else {
            self.adapters
                .iter()
                .map(|a| {
                    if a.vram_mb > 0 {
                        format!("{} ({} MB)", a.name, a.vram_mb)
                    } else {
                        a.name.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join("  +  ")
        };
        v.push(("Graphics", gpus, Act::None));
        let enc = match (&self.phase, self.pipeline) {
            (Phase::Probing(_), _) => "Testing AV1 encoders...".to_string(),
            (_, Some(p)) => format!("{}   (click to re-test)", p.label()),
            _ if self.ffmpeg.is_none() => "ffmpeg.exe not found - reinstall SRec".to_string(),
            _ => "None working   (click to re-test)".to_string(),
        };
        let enc_act = Act::Retest;
        v.push(("Encoder", enc, lock(enc_act)));
        let tgt = match &self.target {
            Target::Display => "Full display   (Ctrl+Shift+S to pick window / area)".to_string(),
            Target::Window { title, .. } => format!("Window: {}   (click for full display)", truncate(title, 40)),
            Target::Area(r) => format!(
                "Area {} x {} at {},{}   (click for full display)",
                r[2], r[3], r[0], r[1]
            ),
        };
        v.push(("Capture", tgt, lock(Act::Target)));
        v.push((
            "Size target",
            encoder::budget_label(self.cfg.mb_per_hour),
            lock(Act::Budget),
        ));
        let res = if self.cfg.max_1080 {
            "Max 1080p - larger screens are scaled down (smaller files)"
        } else {
            "Native - full screen resolution"
        };
        v.push(("Resolution", res.to_string(), lock(Act::Resolution)));
        v.push(("Frame rate", format!("{} fps", self.cfg.fps), lock(Act::Fps)));
        let disp = match self.displays.get(self.cfg.display) {
            Some(d) => format!(
                "{} of {}   ({} x {})",
                self.cfg.display + 1,
                self.displays.len(),
                d.w,
                d.h
            ),
            None => "Primary".to_string(),
        };
        v.push(("Display", disp, lock(Act::Display)));
        let mic = if self.cfg.mic_device.is_empty() {
            "System default".to_string()
        } else {
            self.cfg.mic_device.clone()
        };
        v.push(("Microphone", mic, lock(Act::Mic)));
        let subs = match (&self.whisper, self.cfg.subtitles) {
            (None, _) => ("Not installed - reinstall SRec".to_string(), Act::None),
            (Some(_), true) => (
                "On - English .srt written live, saved with the video (offline)".to_string(),
                Act::Subtitles,
            ),
            (Some(_), false) => ("Off".to_string(), Act::Subtitles),
        };
        v.push(("Subtitles", subs.0, subs.1));
        v.push((
            "Recordings",
            format!("{}   (open)", self.cfg.out_dir.display()),
            Act::Folder,
        ));
        v.push((
            "Hotkeys",
            "Ctrl+Shift+R  record / stop     Ctrl+Shift+S  select".to_string(),
            Act::None,
        ));
        v.push((
            "About",
            format!(
                "SRec v{}  ·  MIT licence  ·  works fully offline   (licences)",
                env!("CARGO_PKG_VERSION")
            ),
            Act::Licenses,
        ));
        v
    }

    // ---- actions ----------------------------------------------------------------

    fn apply(&mut self, a: Act, ctx: &egui::Context) {
        match a {
            Act::None => {}
            Act::Retest => self.phase = Phase::Probing(spawn_probe(&self.cfg, true, ctx.clone())),
            Act::Licenses => platform::open_install_dir(),
            Act::Budget => {
                self.cfg.mb_per_hour = encoder::next_budget(self.cfg.mb_per_hour);
                self.cfg.save();
            }
            Act::Resolution => {
                self.cfg.max_1080 = !self.cfg.max_1080;
                self.cfg.save();
            }
            Act::Fps => {
                self.cfg.fps = match self.cfg.fps {
                    15 => 25,
                    25 => 30,
                    30 => 60,
                    _ => 15,
                };
                self.cfg.save();
            }
            Act::Display => {
                if !self.displays.is_empty() {
                    self.cfg.display = (self.cfg.display + 1) % self.displays.len();
                    self.target = Target::Display;
                    self.cfg.save();
                }
            }
            Act::Mic => {
                if !self.mics.is_empty() {
                    let idx = self.mics.iter().position(|n| *n == self.cfg.mic_device);
                    self.cfg.mic_device = match idx {
                        None => self.mics[0].clone(),
                        Some(i) if i + 1 < self.mics.len() => self.mics[i + 1].clone(),
                        Some(_) => String::new(),
                    };
                    self.cfg.save();
                }
            }
            Act::Target => self.target = Target::Display,
            Act::Folder => platform::open_folder(&self.cfg.out_dir),
            Act::Subtitles => {
                if self.whisper.is_none() {
                    self.toast("Subtitles are not installed - reinstall SRec");
                    return;
                }
                self.cfg.subtitles = !self.cfg.subtitles;
                self.cfg.save();
                let msg = match &self.phase {
                    Phase::Recording(s) if s.has_subtitles() => {
                        s.ctl.subs_on.store(self.cfg.subtitles, Ordering::Relaxed);
                        if self.cfg.subtitles {
                            "Subtitles on"
                        } else {
                            "Subtitles paused for this recording"
                        }
                    }
                    Phase::Recording(_) if self.cfg.subtitles => "Subtitles will start with the next recording",
                    _ if self.cfg.subtitles => "Subtitles on - English .srt with each video",
                    _ => "Subtitles off",
                };
                self.toast(msg);
            }
        }
    }

    fn on_status_click(&mut self, ctx: &egui::Context) {
        if matches!(self.phase, Phase::MissingFfmpeg) {
            self.phase = Phase::Probing(spawn_probe(&self.cfg, true, ctx.clone()));
        } else if matches!(self.phase, Phase::Failed(_)) {
            self.settings_open = true;
        } else if matches!(self.phase, Phase::Ready) {
            let fresh = self
                .toast
                .as_ref()
                .map(|(_, t)| t.elapsed() < Duration::from_secs(5))
                .unwrap_or(false);
            if let (true, Some(p)) = (fresh, &self.last_saved) {
                platform::reveal(p);
            } else if let Some(p) = &self.last_saved {
                platform::reveal(p);
            } else {
                platform::open_folder(&self.cfg.out_dir);
            }
        }
    }

    // ---- window geometry ------------------------------------------------------

    fn apply_size(&mut self, ctx: &egui::Context, target: Vec2) {
        if !self.positioned {
            if let Some(mon) = ctx.input(|i| i.viewport().monitor_size) {
                ctx.send_viewport_cmd(ViewportCommand::OuterPosition(pos2((mon.x - target.x) / 2.0, 10.0)));
                self.positioned = true;
            }
        }
        if (target - self.cur_size).length() > 0.5 {
            let outer = ctx.input(|i| i.viewport().outer_rect);
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(target));
            if let Some(r) = outer {
                let cx = r.center().x;
                ctx.send_viewport_cmd(ViewportCommand::OuterPosition(pos2(cx - target.x / 2.0, r.min.y)));
            }
            self.cur_size = target;
        }
    }

    // ---- drawing --------------------------------------------------------------

    fn draw(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, expanded: bool, rows: &[(&'static str, String, Act)]) {
        let full = ui.max_rect();
        let p = ui.painter().clone();
        let radius = if self.settings_open {
            22.0
        } else {
            full.height().min(ROW_H) / 2.0
        };
        p.rect_filled(
            full,
            Rounding::same(radius),
            Color32::from_rgba_unmultiplied(14, 14, 16, 246),
        );

        let bg = ui.interact(full, egui::Id::new("bg"), Sense::click_and_drag());
        if bg.drag_started() {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        }

        let row = Rect::from_min_size(full.min, vec2(full.width(), ROW_H));
        let cy = row.center().y;
        let t = ctx.input(|i| i.time) as f32;

        // status dot
        let (dot, pulse) = match &self.phase {
            Phase::Recording(_) => (RED, true),
            Phase::Probing(_) | Phase::Saving(_) => (AMBER, true),
            Phase::Ready => (GREEN, false),
            _ => (AMBER, false),
        };
        let r = if pulse { 5.0 + 1.3 * (t * 5.0).sin() } else { 5.0 };
        p.circle_filled(pos2(row.min.x + 22.0, cy), r, dot);

        // status text
        let text_right = if expanded { row.max.x - 274.0 } else { row.max.x - 16.0 };
        let text_rect = Rect::from_min_max(pos2(row.min.x + 34.0, row.min.y), pos2(text_right, row.max.y));
        let (status, col) = self.status_text();
        p.with_clip_rect(text_rect).text(
            pos2(text_rect.min.x, cy),
            Align2::LEFT_CENTER,
            status,
            FontId::proportional(14.0),
            col,
        );
        let st = ui.interact(text_rect, egui::Id::new("status"), Sense::click_and_drag());
        if st.drag_started() {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        }
        if st.clicked() {
            self.on_status_click(ctx);
        }

        if expanded {
            let recording = matches!(self.phase, Phase::Recording(_));
            let can_record = matches!(self.phase, Phase::Ready | Phase::Recording(_));
            let mut x = row.max.x - 24.0;

            let c_close = pos2(x, cy);
            let close = button(ui, "close", c_close);
            draw_close(&p, c_close, DIM);
            x -= 36.0;

            let c_gear = pos2(x, cy);
            let gear = button(ui, "gear", c_gear);
            draw_gear(&p, c_gear, if self.settings_open { TXT } else { DIM });
            x -= 36.0;

            let c_cc = pos2(x, cy);
            let cc = button(ui, "cc", c_cc);
            draw_cc(&p, c_cc, self.cfg.subtitles && self.whisper.is_some());
            x -= 36.0;

            let c_spk = pos2(x, cy);
            let spk = button(ui, "spk", c_spk);
            draw_speaker(&p, c_spk, self.cfg.spk_enabled);
            x -= 36.0;

            let c_mic = pos2(x, cy);
            let mic = button(ui, "mic", c_mic);
            draw_mic(&p, c_mic, self.cfg.mic_enabled);
            if let Phase::Recording(s) = &self.phase {
                let lvl = s.ctl.mic_level().clamp(0.0, 1.0).sqrt();
                let bar = Rect::from_min_size(pos2(c_mic.x - 9.0, c_mic.y + 12.0), vec2(18.0 * lvl, 2.5));
                p.rect_filled(bar, Rounding::same(1.0), GREEN);
            }
            x -= 36.0;

            let c_sel = pos2(x, cy);
            let sel = button(ui, "select", c_sel);
            draw_select(
                &p,
                c_sel,
                if matches!(self.target, Target::Display) {
                    DIM
                } else {
                    BLUE
                },
            );
            x -= 40.0;

            let c_rec = pos2(x, cy);
            let rec = button(ui, "rec", c_rec);
            draw_record(&p, c_rec, recording, can_record);

            if rec.clicked() {
                self.toggle_record();
            }
            if sel.clicked() {
                if self.is_busy() {
                    self.toast("Stop the recording to change the capture area");
                } else {
                    self.start_selection();
                }
            }
            if mic.clicked() {
                self.cfg.mic_enabled = !self.cfg.mic_enabled;
                if let Phase::Recording(s) = &self.phase {
                    s.ctl.mic_on.store(self.cfg.mic_enabled, Ordering::Relaxed);
                }
                self.cfg.save();
            }
            if cc.clicked() {
                self.apply(Act::Subtitles, ctx);
            }
            if spk.clicked() {
                self.cfg.spk_enabled = !self.cfg.spk_enabled;
                if let Phase::Recording(s) = &self.phase {
                    s.ctl.spk_on.store(self.cfg.spk_enabled, Ordering::Relaxed);
                }
                self.cfg.save();
            }
            if gear.clicked() {
                self.settings_open = !self.settings_open;
            }
            if close.clicked() {
                if self.is_busy() {
                    self.toast("Stop the recording before closing");
                } else {
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                }
            }
        }

        if self.settings_open && !rows.is_empty() {
            p.line_segment(
                [pos2(full.min.x + 16.0, row.max.y), pos2(full.max.x - 16.0, row.max.y)],
                Stroke::new(1.0_f32, Color32::from_white_alpha(28)),
            );
            let mut clicked = Act::None;
            let mut y = row.max.y + 6.0;
            for (i, (label, value, act)) in rows.iter().enumerate() {
                let r = Rect::from_min_size(pos2(full.min.x + 10.0, y), vec2(full.width() - 20.0, SET_ROW - 2.0));
                let resp = ui.interact(r, egui::Id::new(("row", i)), Sense::click());
                if *act != Act::None && resp.hovered() {
                    p.rect_filled(r, Rounding::same(6.0), Color32::from_white_alpha(18));
                }
                p.text(
                    pos2(r.min.x + 8.0, r.center().y),
                    Align2::LEFT_CENTER,
                    *label,
                    FontId::proportional(12.5),
                    DIM,
                );
                let vr = Rect::from_min_max(pos2(r.min.x + 96.0, r.min.y), pos2(r.max.x - 8.0, r.max.y));
                let vc = if *act != Act::None {
                    TXT
                } else {
                    Color32::from_rgb(200, 200, 210)
                };
                p.with_clip_rect(vr).text(
                    pos2(vr.min.x, vr.center().y),
                    Align2::LEFT_CENTER,
                    value,
                    FontId::proportional(12.5),
                    vc,
                );
                if resp.clicked() {
                    clicked = *act;
                }
                y += SET_ROW;
            }
            self.apply(clicked, ctx);
        }
    }
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Hide the island from captures (retry a few frames until the window exists).
        if self.excluded_tries < 30 {
            self.excluded_tries = if platform::exclude_from_capture() {
                999
            } else {
                self.excluded_tries + 1
            };
        }

        self.poll();
        if self.hk_record.swap(false, Ordering::SeqCst) {
            self.toggle_record();
        }
        if self.hk_select.swap(false, Ordering::SeqCst) {
            self.start_selection();
        }

        if ctx.input(|i| i.pointer.hover_pos().is_some()) {
            self.last_hover = Instant::now();
        }
        let expanded = self.settings_open || self.last_hover.elapsed() < Duration::from_millis(600);
        let rows = if self.settings_open {
            self.settings_rows()
        } else {
            Vec::new()
        };
        let size = if self.settings_open {
            vec2(W_BIG, ROW_H + 12.0 + rows.len() as f32 * SET_ROW)
        } else if expanded {
            vec2(W_BIG, ROW_H)
        } else {
            vec2(W_SMALL, ROW_H)
        };
        self.apply_size(ctx, size);

        egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show(ctx, |ui| self.draw(ui, ctx, expanded, &rows));

        let ms = match self.phase {
            Phase::Recording(_) | Phase::Probing(_) | Phase::Saving(_) => 100,
            _ => 300,
        };
        ctx.request_repaint_after(Duration::from_millis(ms));
    }
}

// ---- icon drawing ------------------------------------------------------------

fn button(ui: &mut egui::Ui, id: &str, c: Pos2) -> egui::Response {
    let rect = Rect::from_center_size(c, vec2(32.0, 32.0));
    let resp = ui.interact(rect, egui::Id::new(id), Sense::click());
    if resp.hovered() {
        ui.painter().circle_filled(c, 16.0, Color32::from_white_alpha(30));
    }
    resp
}

fn draw_record(p: &Painter, c: Pos2, recording: bool, enabled: bool) {
    let col = if enabled { RED } else { Color32::from_rgb(90, 50, 50) };
    if recording {
        p.rect_filled(Rect::from_center_size(c, vec2(12.0, 12.0)), Rounding::same(2.5), col);
    } else {
        p.circle_stroke(c, 10.0, Stroke::new(1.5_f32, Color32::from_white_alpha(120)));
        p.circle_filled(c, 7.0, col);
    }
}

fn draw_mic(p: &Painter, c: Pos2, on: bool) {
    let col = if on { TXT } else { DIM };
    let s = Stroke::new(1.6_f32, col);
    p.rect_filled(
        Rect::from_center_size(c + vec2(0.0, -3.5), vec2(6.0, 10.0)),
        Rounding::same(3.0),
        col,
    );
    let pts: Vec<Pos2> = (0..=12)
        .map(|i| {
            let a = std::f32::consts::PI * (i as f32 / 12.0);
            pos2(c.x - 6.0 * a.cos(), c.y - 1.0 + 5.0 * a.sin())
        })
        .collect();
    p.add(Shape::line(pts, s));
    p.line_segment([pos2(c.x, c.y + 4.0), pos2(c.x, c.y + 7.5)], s);
    p.line_segment([pos2(c.x - 3.5, c.y + 7.5), pos2(c.x + 3.5, c.y + 7.5)], s);
    if !on {
        p.line_segment(
            [pos2(c.x - 8.0, c.y - 9.0), pos2(c.x + 8.0, c.y + 9.0)],
            Stroke::new(2.0_f32, RED),
        );
    }
}

/// "CC" (closed captions) badge for the subtitles toggle.
fn draw_cc(p: &Painter, c: Pos2, on: bool) {
    let col = if on { TXT } else { DIM };
    let r = Rect::from_center_size(c, vec2(20.0, 14.0));
    p.rect_stroke(r, Rounding::same(3.0), Stroke::new(1.5_f32, col));
    p.text(c, Align2::CENTER_CENTER, "CC", FontId::proportional(9.5), col);
    if !on {
        p.line_segment(
            [pos2(c.x - 9.0, c.y - 9.0), pos2(c.x + 9.0, c.y + 9.0)],
            Stroke::new(1.8_f32, DIM),
        );
    }
}

fn draw_speaker(p: &Painter, c: Pos2, on: bool) {
    let col = if on { TXT } else { DIM };
    let o = c + vec2(-3.0, 0.0);
    p.rect_filled(
        Rect::from_min_max(pos2(o.x - 6.0, o.y - 3.0), pos2(o.x - 2.0, o.y + 3.0)),
        Rounding::same(0.5),
        col,
    );
    p.add(Shape::convex_polygon(
        vec![
            pos2(o.x - 2.5, o.y - 3.0),
            pos2(o.x + 3.0, o.y - 7.0),
            pos2(o.x + 3.0, o.y + 7.0),
            pos2(o.x - 2.5, o.y + 3.0),
        ],
        col,
        Stroke::NONE,
    ));
    if on {
        for r in [5.0f32, 9.0] {
            let pts: Vec<Pos2> = (-4..=4)
                .map(|i| {
                    let a = i as f32 * 0.17;
                    pos2(o.x + 3.0 + r * a.cos(), o.y + r * a.sin())
                })
                .collect();
            p.add(Shape::line(pts, Stroke::new(1.5_f32, col)));
        }
    } else {
        p.line_segment(
            [pos2(c.x - 8.0, c.y - 9.0), pos2(c.x + 8.0, c.y + 9.0)],
            Stroke::new(2.0_f32, RED),
        );
    }
}

fn draw_select(p: &Painter, c: Pos2, col: Color32) {
    // dashed-corner frame = "pick window / area"
    let s = Stroke::new(1.8_f32, col);
    let (hw, hh, k) = (9.0, 7.0, 4.0);
    for (sx, sy) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        let corner = pos2(c.x + sx * hw, c.y + sy * hh);
        p.line_segment([corner, corner + vec2(-sx * k, 0.0)], s);
        p.line_segment([corner, corner + vec2(0.0, -sy * k)], s);
    }
    p.circle_filled(c, 2.0, col);
}

fn draw_gear(p: &Painter, c: Pos2, col: Color32) {
    p.circle_stroke(c, 4.5, Stroke::new(2.0_f32, col));
    for i in 0..8 {
        let a = i as f32 * std::f32::consts::FRAC_PI_4;
        let d = vec2(a.cos(), a.sin());
        p.line_segment([c + d * 6.0, c + d * 8.5], Stroke::new(2.4_f32, col));
    }
}

fn draw_close(p: &Painter, c: Pos2, col: Color32) {
    let s = Stroke::new(1.8_f32, col);
    p.line_segment([c + vec2(-5.0, -5.0), c + vec2(5.0, 5.0)], s);
    p.line_segment([c + vec2(5.0, -5.0), c + vec2(-5.0, 5.0)], s);
}
