//! Small Windows helpers: capture exclusion, global hotkey, explorer, time.

use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use windows::core::{w, PCWSTR};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetMessageW, SetWindowDisplayAffinity, MSG, WDA_EXCLUDEFROMCAPTURE, WM_HOTKEY,
};

/// Keep the floating island out of the recording (Windows 10 2004+).
pub fn exclude_from_capture() -> bool {
    unsafe {
        match FindWindowW(PCWSTR::null(), w!("SRec")) {
            Ok(h) => SetWindowDisplayAffinity(h, WDA_EXCLUDEFROMCAPTURE).is_ok(),
            Err(_) => false,
        }
    }
}

pub const HOTKEY_RECORD: i32 = 1;
pub const HOTKEY_SELECT: i32 = 2;

/// Ctrl+Shift+R = start/stop, Ctrl+Shift+S = select window/area.
pub fn spawn_hotkeys(record: Arc<AtomicBool>, select: Arc<AtomicBool>, ctx: eframe::egui::Context) {
    std::thread::spawn(move || unsafe {
        let mods = MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT;
        let _ = RegisterHotKey(None, HOTKEY_RECORD, mods, 0x52); // R
        let _ = RegisterHotKey(None, HOTKEY_SELECT, mods, 0x53); // S
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            if msg.message == WM_HOTKEY {
                match msg.wParam.0 as i32 {
                    HOTKEY_RECORD => record.store(true, Ordering::SeqCst),
                    HOTKEY_SELECT => select.store(true, Ordering::SeqCst),
                    _ => {}
                }
                ctx.request_repaint();
            }
        }
    });
}

pub fn timestamp() -> String {
    let t = unsafe { GetLocalTime() };
    format!(
        "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
    )
}

pub fn open_folder(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    let _ = Command::new("explorer").arg(dir).spawn();
}

pub fn reveal(file: &Path) {
    let _ = Command::new("explorer")
        .raw_arg(format!("/select,\"{}\"", file.display()))
        .spawn();
}

/// Opens the install folder (LICENSE, TERMS-OF-USE, THIRD-PARTY-NOTICES).
pub fn open_install_dir() {
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.to_path_buf()))
    {
        let _ = Command::new("explorer").arg(dir).spawn();
    }
}
