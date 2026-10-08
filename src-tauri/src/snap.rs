//! Windows 11 Snap Layouts on our own maximize button. Windows offers the layouts to a window whose hit test says
//! "maximize button" (HTMAXBUTTON) under the mouse, but the page is a WebView2 child window covering the whole
//! client area, so ours is never asked. A small invisible child window sits exactly over the page's maximize
//! button (the page reports where it is), above the WebView: it answers HTMAXBUTTON, maximizes or restores on a
//! click, and tells the page when the mouse is over it so the button still lights up. Windows also wants a strip of
//! non-client area at the top before it offers the layouts from a child window: while the window isn't
//! maximized, one pixel is kept, which the window's border covers. (The same approach as Windows Terminal,
//! VS Code and Electron.)

use parking_lot::Mutex;
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
/// The window class, registered with the first window (Ok) or why it couldn't be (the maximize buttons then work
/// without the layouts).
static CLASS: OnceLock<Result<(), String>> = OnceLock::new();

/// One app window's overlay.
struct Overlay {
    /// The app window (its HWND) and its label, for the hover event.
    parent: isize,
    label: String,
    overlay: isize,
    hover: bool,
    pressed: bool,
}

static OVERLAYS: Mutex<Vec<Overlay>> = Mutex::new(Vec::new());

/// Runs `f` on the overlay `hwnd` belongs to (the overlay itself or its app window).
fn with<R>(hwnd: isize, f: impl FnOnce(&mut Overlay) -> R) -> Option<R> {
    OVERLAYS.lock().iter_mut().find(|o| o.overlay == hwnd || o.parent == hwnd).map(f)
}

/// Tells that window's page whether the mouse is over its maximize button.
fn hover(o: &mut Overlay, on: bool) {
    if o.hover != on {
        o.hover = on;
        if let Some(app) = APP.get() {
            let _ = app.emit_to(o.label.as_str(), "snap-hover", on);
        }
    }
}

unsafe extern "system" fn overlay_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let me = hwnd.0 as isize;
    match msg {
        WM_NCHITTEST => LRESULT(HTMAXBUTTON as isize),
        WM_NCMOUSEMOVE => {
            if with(me, |o| o.hover) == Some(false) {
                let mut t = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE | TME_NONCLIENT, hwndTrack: hwnd, dwHoverTime: 0 };
                let _ = TrackMouseEvent(&mut t);
            }
            with(me, |o| hover(o, true));
            LRESULT(0)
        }
        WM_NCMOUSELEAVE => {
            with(me, |o| {
                o.pressed = false;
                hover(o, false);
            });
            LRESULT(0)
        }
        // Handled here: the default would start the system's own button tracking for a window without one.
        WM_NCLBUTTONDOWN => {
            with(me, |o| o.pressed = true);
            LRESULT(0)
        }
        WM_NCLBUTTONUP => {
            let parent = with(me, |o| {
                let was = std::mem::take(&mut o.pressed);
                hover(o, false);
                was.then_some(o.parent)
            });
            // Outside the lock: maximizing sends the window messages that may come back here.
            if let Some(Some(parent)) = parent {
                let parent = HWND(parent as *mut _);
                let _ = ShowWindow(parent, if IsZoomed(parent).as_bool() { SW_RESTORE } else { SW_MAXIMIZE });
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// An app window: one pixel of non-client area at the top while it isn't maximized (see the top).
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

/// Puts an overlay on an app window (`parent`, labelled `label`), when the window is made.
pub fn install(app: &AppHandle, parent: isize, label: &str) -> Result<(), String> {
    let _ = APP.set(app.clone());
    CLASS.get_or_init(register).clone()?;
    // SAFETY: plain Win32 window creation on the UI thread (windows are made there); the overlay is a child of the
    // app window and goes with it. The subclass stays for the window's life.
    unsafe {
        let instance = GetModuleHandleW(None).map_err(|e| format!("module: {e}"))?;
        let parent_hwnd = HWND(parent as *mut _);
        let overlay = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_NOACTIVATE,
            w!("PaginautSnapButton"),
            w!(""),
            WS_CHILD | WS_CLIPSIBLINGS,
            0,
            0,
            0,
            0,
            Some(parent_hwnd),
            None,
            Some(instance.into()),
            None,
        )
        .map_err(|e| format!("window: {e}"))?;
        // Next to invisible: a window that's fully transparent isn't hit-tested at all.
        SetLayeredWindowAttributes(overlay, Default::default(), 1, LWA_ALPHA).map_err(|e| format!("layered: {e}"))?;
        OVERLAYS.lock().push(Overlay { parent, label: label.to_owned(), overlay: overlay.0 as isize, hover: false, pressed: false });
        let _ = SetWindowSubclass(parent_hwnd, Some(main_proc), 0x5041_4749, 0);
        // The frame is worked out again with the strip.
        let _ = SetWindowPos(parent_hwnd, None, 0, 0, 0, 0, windows::Win32::UI::WindowsAndMessaging::SWP_FRAMECHANGED
            | windows::Win32::UI::WindowsAndMessaging::SWP_NOMOVE
            | windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE
            | windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER
            | SWP_NOACTIVATE);
    }
    Ok(())
}

fn register() -> Result<(), String> {
    // SAFETY: registers our overlay's window class once, for the app's life.
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
    }
    Ok(())
}

/// The app window `label` is gone (its overlay went with it).
pub fn forget(label: &str) {
    OVERLAYS.lock().retain(|o| o.label != label);
}

/// Where the page's maximize button is, in physical pixels of the window's client area; hidden when `shown` is false
/// (full screen, Book mode).
pub fn place(parent: isize, x: i32, y: i32, w: i32, h: i32, shown: bool) -> Result<(), String> {
    let Some(overlay) = with(parent, |o| o.overlay) else {
        return Err(match CLASS.get() {
            Some(Err(e)) => e.clone(),
            _ => "not set up".into(),
        });
    };
    let overlay = HWND(overlay as *mut _);
    // SAFETY: our own window, created in install; kept above the WebView.
    unsafe {
        if shown && w > 0 && h > 0 {
            let _ = SetWindowPos(overlay, Some(HWND_TOP), x, y, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        } else {
            let _ = SetWindowPos(overlay, None, 0, 0, 0, 0, SWP_NOACTIVATE | SWP_HIDEWINDOW);
            with(parent, |o| hover(o, false));
        }
    }
    Ok(())
}
