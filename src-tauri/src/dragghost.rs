//! The tab being dragged, shown under the mouse even outside Paginaut's windows: a small borderless window that
//! follows the pointer (the page can only draw inside its own window). It never takes the mouse or the focus.
//! Windows only; elsewhere the page draws its own, inside the window.

use parking_lot::Mutex;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::OnceLock;
use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateRoundRectRgn, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect, GetDC,
    GetTextExtentPoint32W, InvalidateRect, ReleaseDC, SelectObject, SetBkMode, SetTextColor, SetWindowRgn, CLEARTYPE_QUALITY,
    CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, HDC, HFONT, OUT_DEFAULT_PRECIS,
    PAINTSTRUCT, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, KillTimer, RegisterClassW, SetLayeredWindowAttributes, SetTimer,
    SetWindowPos, ShowWindow, HWND_TOPMOST, LWA_ALPHA, SWP_NOACTIVATE, SWP_NOSIZE, SW_SHOWNOACTIVATE, WM_PAINT, WM_TIMER, WNDCLASSW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

/// What the ghost says and its colours (the app's theme: panel, text, muted text, accent).
struct Look {
    name: Vec<u16>,
    hint: Vec<u16>,
    colors: [COLORREF; 4],
}

static LOOK: Mutex<Option<Look>> = Mutex::new(None);
static GHOST: AtomicIsize = AtomicIsize::new(0);
static CLASS: OnceLock<bool> = OnceLock::new();

const TIMER: usize = 1;
/// Base sizes, in 96-dpi pixels.
const NAME_PT: i32 = 13;
const HINT_PT: i32 = 12;
const PAD_X: i32 = 12;
const PAD_Y: i32 = 8;
const BAR: i32 = 3;
const MAX_TEXT: i32 = 300;

fn scaled(hwnd: HWND, v: i32) -> i32 {
    // SAFETY: plain query on our own window.
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as i32;
    v * dpi / 96
}

fn font(hwnd: HWND, px: i32, weight: i32) -> HFONT {
    // SAFETY: a GDI font, deleted by the caller.
    unsafe {
        CreateFontW(-scaled(hwnd, px), 0, 0, 0, weight, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, 0, w!("Segoe UI"))
    }
}

fn text_size(dc: HDC, f: HFONT, text: &[u16]) -> SIZE {
    let mut s = SIZE::default();
    // SAFETY: the DC and font are valid for the call; the old font goes back.
    unsafe {
        let old = SelectObject(dc, f.into());
        let _ = GetTextExtentPoint32W(dc, text, &mut s);
        SelectObject(dc, old);
    }
    s
}

/// The ghost's size for its text, and the height of its first line.
fn measure(hwnd: HWND, look: &Look) -> (i32, i32, i32) {
    // SAFETY: the window's DC, released below; fonts deleted.
    unsafe {
        let dc = GetDC(Some(hwnd));
        let (nf, hf) = (font(hwnd, NAME_PT, 600), font(hwnd, HINT_PT, 400));
        let n = text_size(dc, nf, &look.name);
        let h = if look.hint.is_empty() { SIZE::default() } else { text_size(dc, hf, &look.hint) };
        let _ = DeleteObject(nf.into());
        let _ = DeleteObject(hf.into());
        ReleaseDC(Some(hwnd), dc);
        let text = n.cx.max(h.cx).min(scaled(hwnd, MAX_TEXT));
        let w = scaled(hwnd, BAR + PAD_X * 2) + text;
        let gap = if look.hint.is_empty() { 0 } else { scaled(hwnd, 2) };
        let ht = scaled(hwnd, PAD_Y * 2) + n.cy + gap + h.cy;
        (w, ht, n.cy)
    }
}

fn paint(hwnd: HWND) {
    let look = LOOK.lock();
    let Some(look) = look.as_ref() else { return };
    let (w, h, line) = measure(hwnd, look);
    // SAFETY: standard WM_PAINT drawing with GDI objects made and deleted here.
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut ps);
        let fill = |r: RECT, c: COLORREF| {
            let b = CreateSolidBrush(c);
            FillRect(dc, &r, b);
            let _ = DeleteObject(b.into());
        };
        fill(RECT { left: 0, top: 0, right: w, bottom: h }, look.colors[0]);
        fill(RECT { left: 0, top: 0, right: scaled(hwnd, BAR), bottom: h }, look.colors[3]);
        SetBkMode(dc, TRANSPARENT);
        let x = scaled(hwnd, BAR + PAD_X);
        let y = scaled(hwnd, PAD_Y);
        let draw = |text: &[u16], f: HFONT, c: COLORREF, top: i32, bottom: i32| {
            let old = SelectObject(dc, f.into());
            SetTextColor(dc, c);
            let mut t = text.to_vec();
            let mut r = RECT { left: x, top, right: w - scaled(hwnd, PAD_X), bottom };
            DrawTextW(dc, &mut t, &mut r, DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX | DT_VCENTER);
            SelectObject(dc, old);
            let _ = DeleteObject(f.into());
        };
        draw(&look.name, font(hwnd, NAME_PT, 600), look.colors[1], y, y + line);
        if !look.hint.is_empty() {
            draw(&look.hint, font(hwnd, HINT_PT, 400), look.colors[2], y + line + scaled(hwnd, 2), h - y);
        }
        let _ = EndPaint(hwnd, &ps);
    }
}

/// Keeps the ghost just below and right of the pointer.
fn follow(hwnd: HWND) {
    let mut p = POINT::default();
    // SAFETY: our own window; never activated.
    unsafe {
        if GetCursorPos(&mut p).is_ok() {
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), p.x + scaled(hwnd, 14), p.y + scaled(hwnd, 18), 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        WM_TIMER => {
            follow(hwnd);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn register() -> bool {
    // SAFETY: registers the ghost's window class once, for the app's life.
    unsafe {
        let Ok(instance) = GetModuleHandleW(None) else { return false };
        let class = WNDCLASSW { lpfnWndProc: Some(proc), hInstance: instance.into(), lpszClassName: w!("PaginautDragGhost"), ..Default::default() };
        RegisterClassW(&class) != 0
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// Shows the ghost (or changes what it says). On the UI thread.
pub fn show(name: &str, hint: &str, colors: [[u8; 3]; 4]) -> bool {
    if !*CLASS.get_or_init(register) {
        return false;
    }
    let colors = colors.map(|[r, g, b]| COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16));
    *LOOK.lock() = Some(Look { name: wide(name), hint: wide(hint), colors });
    let mut hwnd = HWND(GHOST.load(Ordering::Relaxed) as *mut _);
    // SAFETY: plain Win32 window on the UI thread: a click-through, never-activated, topmost popup.
    unsafe {
        if hwnd.0.is_null() {
            let Ok(instance) = GetModuleHandleW(None) else { return false };
            let Ok(h) = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                w!("PaginautDragGhost"),
                w!(""),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance.into()),
                None,
            ) else {
                return false;
            };
            hwnd = h;
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 242, LWA_ALPHA);
            SetTimer(Some(hwnd), TIMER, 15, None);
            GHOST.store(hwnd.0 as isize, Ordering::Relaxed);
        }
        let (w, h, _) = measure(hwnd, LOOK.lock().as_ref().unwrap());
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, w, h, SWP_NOACTIVATE);
        let r = scaled(hwnd, 16);
        SetWindowRgn(hwnd, Some(CreateRoundRectRgn(0, 0, w + 1, h + 1, r, r)), true);
        follow(hwnd);
        let _ = InvalidateRect(Some(hwnd), None, true);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    true
}

pub fn hide() {
    let hwnd = GHOST.swap(0, Ordering::Relaxed);
    if hwnd != 0 {
        let hwnd = HWND(hwnd as *mut _);
        // SAFETY: our own window, made in show.
        unsafe {
            let _ = KillTimer(Some(hwnd), TIMER);
            let _ = DestroyWindow(hwnd);
        }
    }
    *LOOK.lock() = None;
}
