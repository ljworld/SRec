//! Settings stored in %APPDATA%\SRec\config.ini

use crate::encoder::{BUDGETS, DEFAULT_BUDGET};
use std::path::PathBuf;

#[derive(Clone)]
pub struct Config {
    pub mb_per_hour: u32,
    pub max_1080: bool,
    pub fps: u32,
    pub display: usize,
    pub mic_enabled: bool,
    pub spk_enabled: bool,
    /// Write English .srt subtitles after each recording.
    pub subtitles: bool,
    pub mic_device: String,
    pub out_dir: PathBuf,
    pub cache_key: String,
    pub cache_pipeline: String,
}

impl Config {
    fn path() -> Option<PathBuf> {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("SRec").join("config.ini"))
    }

    fn default_out() -> PathBuf {
        let home = std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        home.join("Videos").join("SRec Recordings")
    }

    pub fn load() -> Self {
        let mut c = Self {
            mb_per_hour: DEFAULT_BUDGET,
            max_1080: true,
            fps: 30,
            display: 0,
            mic_enabled: true,
            spk_enabled: true,
            subtitles: true,
            mic_device: String::new(),
            out_dir: Self::default_out(),
            cache_key: String::new(),
            cache_pipeline: String::new(),
        };
        let Some(p) = Self::path() else { return c };
        // Settings from before the rename (AV1 Island) are picked up once;
        // the next save writes them to the new location.
        let old = std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("AV1Island").join("config.ini"));
        let text = std::fs::read_to_string(&p).or_else(|e| old.map(std::fs::read_to_string).unwrap_or(Err(e)));
        let Ok(text) = text else { return c };
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let v = v.trim();
            match k.trim() {
                "mb_per_hour" => {
                    if let Ok(n) = v.parse::<u32>() {
                        if BUDGETS.contains(&n) {
                            c.mb_per_hour = n;
                        }
                    }
                }
                "max_1080" => c.max_1080 = v != "0",
                "fps" => {
                    if let Ok(n) = v.parse::<u32>() {
                        if [15, 25, 30, 60].contains(&n) {
                            c.fps = n;
                        }
                    }
                }
                "display" => c.display = v.parse().unwrap_or(0),
                "mic_enabled" => c.mic_enabled = v == "1",
                "spk_enabled" => c.spk_enabled = v == "1",
                "subtitles" => c.subtitles = v == "1",
                "mic_device" => c.mic_device = v.to_string(),
                "out_dir" if !v.is_empty() => c.out_dir = PathBuf::from(v),
                "cache_key" => c.cache_key = v.to_string(),
                "cache_pipeline" => c.cache_pipeline = v.to_string(),
                _ => {}
            }
        }
        c
    }

    pub fn save(&self) {
        let Some(p) = Self::path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let b = |x: bool| if x { "1" } else { "0" };
        let text = format!(
            "mb_per_hour={}\nmax_1080={}\nfps={}\ndisplay={}\nmic_enabled={}\nspk_enabled={}\nsubtitles={}\nmic_device={}\nout_dir={}\ncache_key={}\ncache_pipeline={}\n",
            self.mb_per_hour,
            b(self.max_1080),
            self.fps,
            self.display,
            b(self.mic_enabled),
            b(self.spk_enabled),
            b(self.subtitles),
            self.mic_device,
            self.out_dir.display(),
            self.cache_key,
            self.cache_pipeline
        );
        let _ = std::fs::write(p, text);
    }
}
