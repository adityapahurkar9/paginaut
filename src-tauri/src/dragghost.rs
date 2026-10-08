//! The tab being dragged, shown under the mouse even outside Paginaut's windows: a small pill that follows the
//! pointer (the page can only draw inside its own window), with an icon for what letting go does: a + (accent
//! coloured) over another window's tabs, a new-window icon off the tabs. It never takes the mouse or the focus.
//! Windows only; elsewhere the page draws its own label, inside the window.

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

/// What letting go does, shown by the pill's icon (and colour).
#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    /// Moving along its own tabs.
    Move,
    /// Joins the window under the mouse (accent coloured, a +).
    Join,
    /// Opens a new window.
    New,
}

/// What the ghost says and its colours (the app's theme: panel, text, accent, text on accent).
struct Look {
    name: Vec<u16>,
    kind: Kind,
    colors: [COLORREF; 4],
}

static LOOK: Mutex<Option<Look>> = Mutex::new(None);
static GHOST: AtomicIsize = AtomicIsize::new(0);
static CLASS: OnceLock<bool> = OnceLock::new();

const TIMER: usize = 1;
/// Base sizes, in 96-dpi pixels.
const TEXT_PX: i32 = 12;
const ICON_PX: i32 = 12;
const PAD_X: i32 = 9;
const PAD_Y: i32 = 5;
const GAP: i32 = 7;
const MAX_TEXT: i32 = 200;

/// Segoe MDL2 Assets icons (on Windows 10 and 11): Document, Add, Open in new window.
fn glyph(kind: Kind) -> u16 {
    match kind {
        Kind::Move => 0xE8A5,
        Kind::Join => 0xE710,
        Kind::New => 0xE8A7,
    }
}

fn scaled(hwnd: HWND, v: i32) -> i32 {
    // SAFETY: plain query on our own window.
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as i32;
    v * dpi / 96
}

fn font(hwnd: HWND, px: i32, weight: i32, icons: bool) -> HFONT {
    let face = if icons { w!("Segoe MDL2 Assets") } else { w!("Segoe UI") };
    // SAFETY: a GDI font, deleted by the caller.
    unsafe { CreateFontW(-scaled(hwnd, px), 0, 0, 0, weight, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, 0, face) }
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

/// The pill's size for its text, and the icon's width.
fn measure(hwnd: HWND, look: &Look) -> (i32, i32, i32) {
    // SAFETY: the window's DC, released below; fonts deleted.
    unsafe {
        let dc = GetDC(Some(hwnd));
        let (tf, gf) = (font(hwnd, TEXT_PX, 600, false), font(hwnd, ICON_PX, 400, true));
        let n = text_size(dc, tf, &look.name);
        let g = text_size(dc, gf, &[glyph(look.kind)]);
        let _ = DeleteObject(tf.into());
        let _ = DeleteObject(gf.into());
        ReleaseDC(Some(hwnd), dc);
        let w = scaled(hwnd, PAD_X * 2 + GAP) + g.cx + n.cx.min(scaled(hwnd, MAX_TEXT));
        let h = scaled(hwnd, PAD_Y * 2) + n.cy.max(g.cy);
        (w, h, g.cx)
    }
}

fn paint(hwnd: HWND) {
    let look = LOOK.lock();
    let Some(look) = look.as_ref() else { return };
    let (w, h, icon_w) = measure(hwnd, look);
    // Joining a window: accent coloured; otherwise the panel's colour with an accent icon.
    let join = look.kind == Kind::Join;
    let (bg, fg, icon) = if join { (look.colors[2], look.colors[3], look.colors[3]) } else { (look.colors[0], look.colors[1], look.colors[2]) };
    // SAFETY: standard WM_PAINT drawing with GDI objects made and deleted here.
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut ps);
        let b = CreateSolidBrush(bg);
        FillRect(dc, &RECT { left: 0, top: 0, right: w, bottom: h }, b);
        let _ = DeleteObject(b.into());
        SetBkMode(dc, TRANSPARENT);
        let draw = |text: &[u16], f: HFONT, c: COLORREF, left: i32, right: i32| {
            let old = SelectObject(dc, f.into());
            SetTextColor(dc, c);
            let mut t = text.to_vec();
            let mut r = RECT { left, top: 0, right, bottom: h };
            DrawTextW(dc, &mut t, &mut r, DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX | DT_VCENTER);
            SelectObject(dc, old);
            let _ = DeleteObject(f.into());
        };
        let x = scaled(hwnd, PAD_X);
        draw(&[glyph(look.kind)], font(hwnd, ICON_PX, 400, true), icon, x, x + icon_w);
        let tx = x + icon_w + scaled(hwnd, GAP);
        draw(&look.name, font(hwnd, TEXT_PX, 600, false), fg, tx, w - scaled(hwnd, PAD_X));
        let _ = EndPaint(hwnd, &ps);
    }
}

/// Keeps the ghost just below and right of the pointer.
fn follow(hwnd: HWND) {
    let mut p = POINT::default();
    // SAFETY: our own window; never activated.
    unsafe {
        if GetCursorPos(&mut p).is_ok() {
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), p.x + scaled(hwnd, 12), p.y + scaled(hwnd, 16), 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
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
pub fn show(name: &str, kind: Kind, colors: [[u8; 3]; 4]) -> bool {
    if !*CLASS.get_or_init(register) {
        return false;
    }
    let colors = colors.map(|[r, g, b]| COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16));
    *LOOK.lock() = Some(Look { name: wide(name), kind, colors });
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
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 245, LWA_ALPHA);
            SetTimer(Some(hwnd), TIMER, 15, None);
            GHOST.store(hwnd.0 as isize, Ordering::Relaxed);
        }
        let (w, h, _) = measure(hwnd, LOOK.lock().as_ref().unwrap());
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, w, h, SWP_NOACTIVATE);
        let r = h;
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
