//! Windows 11 Snap Layouts on our own maximize button. Windows offers the layouts to a window whose hit test says
//! "maximize button" (HTMAXBUTTON) under the mouse, but the page is a WebView2 child window covering the whole
//! client area, so ours is never asked. A small invisible child window sits exactly over the page's maximize
//! button (the page reports where it is), above the WebView: it answers HTMAXBUTTON, maximizes or restores on a
//! click, and tells the page when the mouse is over it so the button still lights up. Windows also wants a strip of
//! non-client area at the top before it offers the layouts from a child window: while the window isn't
//! maximized, one pixel is kept, which the window's border covers. (The same approach as Windows Terminal,
//! VS Code and Electron.)

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::OnceLock;
use tauri::{AppHandle, Emitter};
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, IsZoomed, LoadCursorW, RegisterClassW, SetLayeredWindowAttributes, SetWindowPos, ShowWindow,
    HTMAXBUTTON, HWND_TOP, IDC_ARROW, LWA_ALPHA, NCCALCSIZE_PARAMS, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_MAXIMIZE,
    SW_RESTORE, WM_NCCALCSIZE, WM_NCHITTEST, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WM_NCMOUSELEAVE, WM_NCMOUSEMOVE, WNDCLASSW,
    WS_CHILD, WS_CLIPSIBLINGS, WS_EX_LAYERED, WS_EX_NOACTIVATE,
};

static APP: OnceLock<AppHandle> = OnceLock::new();
static OVERLAY: AtomicIsize = AtomicIsize::new(0);
static MAIN: AtomicIsize = AtomicIsize::new(0);
static HOVER: AtomicBool = AtomicBool::new(false);
static PRESSED: AtomicBool = AtomicBool::new(false);
/// Why the overlay couldn't be made, if it couldn't (the maximize button then works without the layouts).
static FAILED: OnceLock<String> = OnceLock::new();

fn hover(on: bool) {
    if HOVER.swap(on, Ordering::Relaxed) != on {
        if let Some(app) = APP.get() {
            let _ = app.emit("snap-hover", on);
        }
    }
}

unsafe extern "system" fn overlay_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_NCHITTEST => LRESULT(HTMAXBUTTON as isize),
        WM_NCMOUSEMOVE => {
            if !HOVER.load(Ordering::Relaxed) {
                let mut t = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE | TME_NONCLIENT, hwndTrack: hwnd, dwHoverTime: 0 };
                let _ = TrackMouseEvent(&mut t);
            }
            hover(true);
            LRESULT(0)
        }
        WM_NCMOUSELEAVE => {
            PRESSED.store(false, Ordering::Relaxed);
            hover(false);
            LRESULT(0)
        }
        // Handled here: the default would start the system's own button tracking for a window without one.
        WM_NCLBUTTONDOWN => {
            PRESSED.store(true, Ordering::Relaxed);
            LRESULT(0)
        }
        WM_NCLBUTTONUP => {
            if PRESSED.swap(false, Ordering::Relaxed) {
                let main = HWND(MAIN.load(Ordering::Relaxed) as *mut _);
                let _ = ShowWindow(main, if IsZoomed(main).as_bool() { SW_RESTORE } else { SW_MAXIMIZE });
                hover(false);
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// The main window: one pixel of non-client area at the top while it isn't maximized (see the top).
unsafe extern "system" fn main_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _id: usize, _data: usize) -> LRESULT {
    let calc = msg == WM_NCCALCSIZE && wparam.0 != 0;
    // The proposed window rectangle, before the window's own handling turns it into the client area.
    let top = if calc { (*(lparam.0 as *const NCCALCSIZE_PARAMS)).rgrc[0].top } else { 0 };
    let r = DefSubclassProc(hwnd, msg, wparam, lparam);
    if calc && !IsZoomed(hwnd).as_bool() {
        let params = &mut *(lparam.0 as *mut NCCALCSIZE_PARAMS);
        let rect: &mut RECT = &mut params.rgrc[0];
        // Only when none was kept (Tauri keeps one itself on Windows 10 and later).
        if rect.top == top && rect.bottom - rect.top > 1 {
            rect.top += 1;
        }
    }
    r
}

/// Puts the overlay on `main` (the app's window). Once, at start.
pub fn install(app: &AppHandle, main: isize) -> Result<(), String> {
    let r = install_now(app, main);
    if let Err(e) = &r {
        let _ = FAILED.set(e.clone());
    }
    r
}

fn install_now(app: &AppHandle, main: isize) -> Result<(), String> {
    let _ = APP.set(app.clone());
    // SAFETY: plain Win32 window creation on the UI thread (setup runs there); the class and window live as long
    // as the app. The subclass stays for the window's life.
    unsafe {
        let instance = GetModuleHandleW(None).map_err(|e| format!("module: {e}"))?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(overlay_proc),
            hInstance: instance.into(),
            lpszClassName: w!("PaginautSnapButton"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err(format!("class: {}", windows::core::Error::from_thread()));
        }
        let parent = HWND(main as *mut _);
        let overlay = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_NOACTIVATE,
            w!("PaginautSnapButton"),
            w!(""),
            WS_CHILD | WS_CLIPSIBLINGS,
            0,
            0,
            0,
            0,
            Some(parent),
            None,
            Some(instance.into()),
            None,
        )
        .map_err(|e| format!("window: {e}"))?;
        // Next to invisible: a window that's fully transparent isn't hit-tested at all.
        SetLayeredWindowAttributes(overlay, Default::default(), 1, LWA_ALPHA).map_err(|e| format!("layered: {e}"))?;
        OVERLAY.store(overlay.0 as isize, Ordering::Relaxed);
        MAIN.store(main, Ordering::Relaxed);
        let _ = SetWindowSubclass(parent, Some(main_proc), 0x5041_4749, 0);
        // The frame is worked out again with the strip.
        let _ = SetWindowPos(parent, None, 0, 0, 0, 0, windows::Win32::UI::WindowsAndMessaging::SWP_FRAMECHANGED
            | windows::Win32::UI::WindowsAndMessaging::SWP_NOMOVE
            | windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE
            | windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER
            | SWP_NOACTIVATE);
    }
    Ok(())
}

/// Where the page's maximize button is, in physical pixels of the window's client area; hidden when `shown` is false
/// (full screen, Book mode).
pub fn place(x: i32, y: i32, w: i32, h: i32, shown: bool) -> Result<(), String> {
    let overlay = OVERLAY.load(Ordering::Relaxed);
    if overlay == 0 {
        return Err(FAILED.get().cloned().unwrap_or_else(|| "not set up".into()));
    }
    let overlay = HWND(overlay as *mut _);
    // SAFETY: our own window, created in install; kept above the WebView.
    unsafe {
        if shown && w > 0 && h > 0 {
            let _ = SetWindowPos(overlay, Some(HWND_TOP), x, y, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        } else {
            let _ = SetWindowPos(overlay, None, 0, 0, 0, 0, SWP_NOACTIVATE | SWP_HIDEWINDOW);
            hover(false);
        }
    }
    Ok(())
}
