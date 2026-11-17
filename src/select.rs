//! Full-screen selection overlay:
//!   click a window  -> record that window
//!   drag            -> record that area
//!   right-click     -> record the whole display under the pointer
//!   Esc             -> cancel

use std::sync::Mutex;
use windows::core::w;
use windows::Win32::Foundation::{BOOL, COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, TRUE, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateSolidBrush, DeleteDC, DeleteObject,
    DrawTextW, EndPaint, FillRect, FrameRect, GetStockObject, InvalidateRect, SelectObject, SetBkMode, SetTextColor,
    DEFAULT_GUI_FONT, DT_CENTER, DT_SINGLELINE, DT_VCENTER, PAINTSTRUCT, SRCCOPY, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, VK_ESCAPE};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, EnumWindows, GetClientRect, GetMessageW,
    GetSystemMetrics, GetWindowLongW, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow,
    IsWindowVisible, LoadCursorW, PostQuitMessage, RegisterClassW, SetForegroundWindow, SetLayeredWindowAttributes,
    TranslateMessage, GWL_EXSTYLE, IDC_CROSS, LWA_ALPHA, MSG, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEMOVE, WM_PAINT, WM_RBUTTONUP, WNDCLASSW, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
    WS_VISIBLE,
};

/// Rectangles are absolute desktop pixels: [x, y, w, h].
#[derive(Clone, Debug)]
pub enum Selection {
    Cancelled,
    FullDisplay { x: i32, y: i32 },
    Window { hwnd: isize, title: String, rect: [i32; 4] },
    Area([i32; 4]),
}

struct WinInfo {
    hwnd: isize,
    title: String,
    rect: RECT,
}

struct State {
    ox: i32,
    oy: i32,
    wins: Vec<WinInfo>,
    hover: Option<usize>,
    start: Option<(i32, i32)>,
    cur: (i32, i32),
    dragging: bool,
    result: Option<Selection>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn to_xywh(r: &RECT) -> [i32; 4] {
    [r.left, r.top, r.right - r.left, r.bottom - r.top]
}

fn hwnd_of(v: isize) -> HWND {
    HWND(v as _)
}

unsafe fn frame_bounds(h: HWND) -> Option<RECT> {
    let mut r = RECT::default();
    let ok = DwmGetWindowAttribute(
        h,
        DWMWA_EXTENDED_FRAME_BOUNDS,
        &mut r as *mut RECT as *mut _,
        std::mem::size_of::<RECT>() as u32,
    )
    .is_ok();
    if ok || GetWindowRect(h, &mut r).is_ok() {
        Some(r)
    } else {
        None
    }
}

/// Current on-screen rectangle of a previously selected window.
pub fn window_rect(hwnd: isize) -> Option<[i32; 4]> {
    unsafe {
        let h = hwnd_of(hwnd);
        if !IsWindow(h).as_bool() || IsIconic(h).as_bool() {
            return None;
        }
        frame_bounds(h).map(|r| to_xywh(&r))
    }
}

pub fn focus(hwnd: isize) {
    unsafe {
        let _ = SetForegroundWindow(hwnd_of(hwnd));
    }
}

unsafe extern "system" fn enum_cb(h: HWND, lp: LPARAM) -> BOOL {
    let list = &mut *(lp.0 as *mut Vec<WinInfo>);
    if !IsWindowVisible(h).as_bool() || IsIconic(h).as_bool() {
        return TRUE;
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(h, Some(&mut pid as *mut u32));
    if pid == GetCurrentProcessId() {
        return TRUE;
    }
    let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
    if ex & WS_EX_TOOLWINDOW.0 != 0 {
        return TRUE;
    }
    let mut cloaked = 0u32;
    if DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut _, 4).is_ok() && cloaked != 0 {
        return TRUE;
    }
    let Some(r) = frame_bounds(h) else { return TRUE };
    if r.right - r.left < 60 || r.bottom - r.top < 60 {
        return TRUE;
    }
    let mut buf = [0u16; 256];
    let n = GetWindowTextW(h, &mut buf);
    if n <= 0 {
        return TRUE;
    }
    let title = String::from_utf16_lossy(&buf[..n as usize]);
    if title == "Program Manager" {
        return TRUE;
    }
    list.push(WinInfo {
        hwnd: h.0 as isize,
        title,
        rect: r,
    });
    TRUE
}

fn list_windows() -> Vec<WinInfo> {
    let mut v: Vec<WinInfo> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(enum_cb), LPARAM(&mut v as *mut Vec<WinInfo> as isize));
    }
    v
}

fn hit(wins: &[WinInfo], x: i32, y: i32) -> Option<usize> {
    wins.iter()
        .position(|w| x >= w.rect.left && x < w.rect.right && y >= w.rect.top && y < w.rect.bottom)
}

fn norm(a: (i32, i32), b: (i32, i32)) -> RECT {
    RECT {
        left: a.0.min(b.0),
        top: a.1.min(b.1),
        right: a.0.max(b.0),
        bottom: a.1.max(b.1),
    }
}

fn mouse(lp: LPARAM) -> (i32, i32) {
    let x = (lp.0 & 0xFFFF) as u16 as i16 as i32;
    let y = ((lp.0 >> 16) & 0xFFFF) as u16 as i16 as i32;
    (x, y)
}

/// Blocking: shows the overlay and returns the user's choice.
/// Call from a background thread (the overlay runs its own message loop).
pub fn pick() -> Selection {
    unsafe {
        let wins = list_windows();
        let ox = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let oy = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        if let Ok(mut g) = STATE.lock() {
            *g = Some(State {
                ox,
                oy,
                wins,
                hover: None,
                start: None,
                cur: (0, 0),
                dragging: false,
                result: None,
            });
        }

        let Ok(module) = GetModuleHandleW(None) else {
            return Selection::Cancelled;
        };
        let hinst: HINSTANCE = module.into();
        let class = w!("SRecSelector");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinst,
            lpszClassName: class,
            hCursor: LoadCursorW(None, IDC_CROSS).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&wc); // fails harmlessly if already registered

        let hwnd = match CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TOOLWINDOW,
            class,
            w!("SRec selector"),
            WS_POPUP | WS_VISIBLE,
            ox,
            oy,
            vw,
            vh,
            None,
            None,
            hinst,
            None,
        ) {
            Ok(h) => h,
            Err(_) => return Selection::Cancelled,
        };
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 130, LWA_ALPHA);
        let _ = SetForegroundWindow(hwnd);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        STATE
            .lock()
            .ok()
            .and_then(|mut g| g.take())
            .and_then(|s| s.result)
            .unwrap_or(Selection::Cancelled)
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => return LRESULT(1),
        WM_PAINT => {
            paint(hwnd);
            return LRESULT(0);
        }
        WM_MOUSEMOVE => {
            let (x, y) = mouse(lp);
            if let Ok(mut g) = STATE.lock() {
                if let Some(s) = g.as_mut() {
                    s.cur = (x + s.ox, y + s.oy);
                    if let Some(st) = s.start {
                        if (s.cur.0 - st.0).abs() > 4 || (s.cur.1 - st.1).abs() > 4 {
                            s.dragging = true;
                        }
                    }
                    if !s.dragging {
                        s.hover = hit(&s.wins, s.cur.0, s.cur.1);
                    }
                }
            }
            let _ = InvalidateRect(hwnd, None, BOOL(0));
            return LRESULT(0);
        }
        WM_LBUTTONDOWN => {
            let (x, y) = mouse(lp);
            if let Ok(mut g) = STATE.lock() {
                if let Some(s) = g.as_mut() {
                    s.cur = (x + s.ox, y + s.oy);
                    s.start = Some(s.cur);
                    s.dragging = false;
                }
            }
            SetCapture(hwnd);
            return LRESULT(0);
        }
        WM_LBUTTONUP => {
            let _ = ReleaseCapture();
            let mut done = false;
            if let Ok(mut g) = STATE.lock() {
                if let Some(s) = g.as_mut() {
                    if s.dragging {
                        let r = norm(s.start.unwrap_or(s.cur), s.cur);
                        if r.right - r.left >= 64 && r.bottom - r.top >= 64 {
                            s.result = Some(Selection::Area(to_xywh(&r)));
                            done = true;
                        }
                    } else if let Some(i) = s.hover {
                        let wi = &s.wins[i];
                        s.result = Some(Selection::Window {
                            hwnd: wi.hwnd,
                            title: wi.title.clone(),
                            rect: to_xywh(&wi.rect),
                        });
                        done = true;
                    } else {
                        s.result = Some(Selection::FullDisplay { x: s.cur.0, y: s.cur.1 });
                        done = true;
                    }
                    s.start = None;
                    s.dragging = false;
                }
            }
            if done {
                let _ = DestroyWindow(hwnd);
            } else {
                let _ = InvalidateRect(hwnd, None, BOOL(0));
            }
            return LRESULT(0);
        }
        WM_RBUTTONUP => {
            let (x, y) = mouse(lp);
            if let Ok(mut g) = STATE.lock() {
                if let Some(s) = g.as_mut() {
                    s.result = Some(Selection::FullDisplay {
                        x: x + s.ox,
                        y: y + s.oy,
                    });
                }
            }
            let _ = DestroyWindow(hwnd);
            return LRESULT(0);
        }
        WM_KEYDOWN => {
            if wp.0 == VK_ESCAPE.0 as usize {
                if let Ok(mut g) = STATE.lock() {
                    if let Some(s) = g.as_mut() {
                        s.result = Some(Selection::Cancelled);
                    }
                }
                let _ = DestroyWindow(hwnd);
                return LRESULT(0);
            }
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            return LRESULT(0);
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

unsafe fn paint(hwnd: HWND) {
    // Snapshot what to draw (client coordinates) without holding the lock while painting.
    let (sel, label) = {
        let g = STATE.lock();
        match g.as_ref().ok().and_then(|g| (**g).as_ref()) {
            Some(s) => {
                let to_client = |r: RECT| RECT {
                    left: r.left - s.ox,
                    top: r.top - s.oy,
                    right: r.right - s.ox,
                    bottom: r.bottom - s.oy,
                };
                if s.dragging {
                    let r = norm(s.start.unwrap_or(s.cur), s.cur);
                    let txt = format!("Area  {} x {}", r.right - r.left, r.bottom - r.top);
                    (Some(to_client(r)), txt)
                } else if let Some(i) = s.hover {
                    let w = &s.wins[i];
                    let txt = format!(
                        "Window: {}  ({} x {})",
                        w.title,
                        w.rect.right - w.rect.left,
                        w.rect.bottom - w.rect.top
                    );
                    (Some(to_client(w.rect)), txt)
                } else {
                    (None, "Click = full display".to_string())
                }
            }
            None => (None, String::new()),
        }
    };

    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);
    let mut cr = RECT::default();
    let _ = GetClientRect(hwnd, &mut cr);
    let (w, h) = (cr.right, cr.bottom);
    let mem = CreateCompatibleDC(hdc);
    let bmp = CreateCompatibleBitmap(hdc, w, h);
    let old_bmp = SelectObject(mem, bmp);

    let bg = CreateSolidBrush(COLORREF(0x0010_1010));
    FillRect(mem, &cr, bg);
    let _ = DeleteObject(bg);

    SetBkMode(mem, TRANSPARENT);
    SetTextColor(mem, COLORREF(0x00FF_FFFF));
    let old_font = SelectObject(mem, GetStockObject(DEFAULT_GUI_FONT));

    if let Some(r) = sel {
        let fill = CreateSolidBrush(COLORREF(0x0060_6060));
        FillRect(mem, &r, fill);
        let _ = DeleteObject(fill);
        let edge = CreateSolidBrush(COLORREF(0x003A_45FF)); // red accent (BGR)
        for i in 0..3 {
            let rr = RECT {
                left: r.left - i,
                top: r.top - i,
                right: r.right + i,
                bottom: r.bottom + i,
            };
            FrameRect(mem, &rr, edge);
        }
        let _ = DeleteObject(edge);
        let mut lr = RECT {
            left: r.left,
            top: (r.top - 26).max(0),
            right: r.left + 600,
            bottom: (r.top - 4).max(22),
        };
        let mut t: Vec<u16> = label.encode_utf16().collect();
        DrawTextW(mem, &mut t, &mut lr, DT_SINGLELINE | DT_VCENTER);
    }

    let help =
        "Click a window to record it   |   Drag to record an area   |   Right-click = full display   |   Esc = cancel";
    let mut hr = RECT {
        left: 0,
        top: 12,
        right: w,
        bottom: 44,
    };
    let mut t: Vec<u16> = help.encode_utf16().collect();
    DrawTextW(mem, &mut t, &mut hr, DT_CENTER | DT_SINGLELINE | DT_VCENTER);

    SelectObject(mem, old_font);
    let _ = BitBlt(hdc, 0, 0, w, h, mem, 0, 0, SRCCOPY);
    SelectObject(mem, old_bmp);
    let _ = DeleteObject(bmp);
    let _ = DeleteDC(mem);
    let _ = EndPaint(hwnd, &ps);
}
