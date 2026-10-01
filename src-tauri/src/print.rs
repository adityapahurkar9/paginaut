//! Printing through Windows: its own print dialog (printer, pages, copies, the printer's preferences), then
//! each page drawn by MuPDF in bands and handed to the printer as pictures. Pages print as they're shown (in
//! the order and turned as arranged, with comments and form entries), turned to suit the paper and fitted to
//! the area the printer can print, centred.

use crate::engine::mupdf::{render_size, RenderOutcome};
use crate::engine::Doc;
use serde::Deserialize;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{GetLastError, GlobalFree, ERROR_CANCELLED, HWND};
use windows::Win32::Graphics::Gdi::{
    DeleteDC, GetDeviceCaps, SetStretchBltMode, StretchDIBits, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HALFTONE, HDC, HORZRES,
    LOGPIXELSX, LOGPIXELSY, SRCCOPY, VERTRES,
};
use windows::Win32::Storage::Xps::{AbortDoc, EndDoc, EndPage, StartDocW, StartPage, DOCINFOW};
use windows::Win32::UI::Controls::Dialogs::{
    PrintDlgExW, PD_CURRENTPAGE, PD_NOSELECTION, PD_PAGENUMS, PD_RESULT_PRINT, PD_RETURNDC, PD_USEDEVMODECOPIESANDCOLLATE, PRINTDLGEXW, PRINTPAGERANGE,
    START_PAGE_GENERAL,
};

/// A page as shown: its index in the file and the extra turn it's shown with.
#[derive(Deserialize, Clone, Copy)]
pub struct Shown {
    pub page: usize,
    pub rot: i32,
}

/// The finest the pages are drawn for the printer, in dots per inch: finer only makes the job larger.
const MAX_DPI: f32 = 300.0;
/// Rows drawn at a time.
const BAND: i32 = 256;

/// A printer the dialog opened, and the pages chosen (indices into the pages shown). Dropping it closes the printer.
pub struct Chosen {
    dc: HDC,
    pub pages: Vec<usize>,
}

// SAFETY: the printer's DC is used by one thread at a time (the dialog's, then the printing thread's).
unsafe impl Send for Chosen {}

impl Drop for Chosen {
    fn drop(&mut self) {
        // SAFETY: the DC the dialog made for us, deleted once.
        unsafe {
            let _ = DeleteDC(self.dc);
        }
    }
}

/// Shows Windows' print dialog over `owner`. Call it on the window's own (UI) thread: Windows 11's dialog, in a
/// process of its own, never appears for a call from another thread. None when cancelled.
pub fn ask(owner: isize, count: usize, current: usize) -> Result<Option<Chosen>, String> {
    Ok(dialog(owner, count, current)?.map(|(dc, pages)| Chosen { dc, pages }))
}

/// Prints the chosen pages (`pages` as shown); `on_page(done)` after each.
pub fn print(chosen: Chosen, doc: &Doc, name: &str, pages: &[Shown], stop: &AtomicBool, on_page: impl FnMut(usize)) -> Result<usize, String> {
    let list: Vec<Shown> = chosen.pages.iter().filter_map(|&i| pages.get(i).copied()).collect();
    print_pages(chosen.dc, doc, name, None, &list, stop, on_page)
}

/// The print dialog: the printer's DC and the pages chosen (indices into the pages shown), or None if cancelled.
fn dialog(owner: isize, count: usize, current: usize) -> Result<Option<(HDC, Vec<usize>)>, String> {
    let mut ranges = [PRINTPAGERANGE::default(); 32];
    let mut pd = PRINTDLGEXW {
        lStructSize: size_of::<PRINTDLGEXW>() as u32,
        hwndOwner: HWND(owner as *mut _),
        Flags: PD_RETURNDC | PD_NOSELECTION | PD_USEDEVMODECOPIESANDCOLLATE,
        nMaxPageRanges: ranges.len() as u32,
        lpPageRanges: ranges.as_mut_ptr(),
        nMinPage: 1,
        nMaxPage: count.max(1) as u32,
        nCopies: 1,
        nStartPage: START_PAGE_GENERAL,
        ..Default::default()
    };
    // SAFETY: pd and its ranges outlive the call; the DEVMODE/DEVNAMES it hands back are freed here.
    let r = unsafe { PrintDlgExW(&mut pd) };
    unsafe {
        if !pd.hDevMode.is_invalid() {
            let _ = GlobalFree(Some(pd.hDevMode));
        }
        if !pd.hDevNames.is_invalid() {
            let _ = GlobalFree(Some(pd.hDevNames));
        }
    }
    r.map_err(|e| format!("The print dialog couldn't open: {}", e.message()))?;
    if pd.dwResultAction != PD_RESULT_PRINT {
        if !pd.hDC.is_invalid() {
            unsafe {
                let _ = DeleteDC(pd.hDC);
            }
        }
        return Ok(None);
    }
    if pd.hDC.is_invalid() {
        return Err("The printer couldn't be opened.".into());
    }
    let chosen: Vec<usize> = if pd.Flags.contains(PD_CURRENTPAGE) {
        vec![current.min(count.saturating_sub(1))]
    } else if pd.Flags.contains(PD_PAGENUMS) {
        let mut v = Vec::new();
        for r in &ranges[..pd.nPageRanges as usize] {
            let (a, b) = (r.nFromPage.min(r.nToPage), r.nFromPage.max(r.nToPage));
            v.extend((a.max(1)..=b.min(count as u32)).map(|p| p as usize - 1));
        }
        v
    } else {
        (0..count).collect()
    };
    Ok(Some((pd.hDC, chosen)))
}

/// `output`: a file to print into instead (for printers that write files; the tests use it).
fn print_pages(dc: HDC, doc: &Doc, name: &str, output: Option<&str>, pages: &[Shown], stop: &AtomicBool, mut done: impl FnMut(usize)) -> Result<usize, String> {
    let title = HSTRING::from(name);
    let out = output.map(HSTRING::from);
    let info = DOCINFOW {
        cbSize: size_of::<DOCINFOW>() as i32,
        lpszDocName: PCWSTR(title.as_ptr()),
        lpszOutput: out.as_ref().map_or(PCWSTR::null(), |o| PCWSTR(o.as_ptr())),
        ..Default::default()
    };
    // SAFETY: GDI printing on the DC the dialog made; every StartPage has its EndPage, StartDoc its EndDoc or AbortDoc.
    unsafe {
        if StartDocW(dc, &info) <= 0 {
            // A printer that writes files (Microsoft Print to PDF) asks where; cancelling that ends here.
            if GetLastError() == ERROR_CANCELLED {
                return Err("cancelled".into());
            }
            return Err("The printer didn't accept the document.".into());
        }
        SetStretchBltMode(dc, HALFTONE);
        let paper = (GetDeviceCaps(Some(dc), HORZRES), GetDeviceCaps(Some(dc), VERTRES));
        let dpi = (GetDeviceCaps(Some(dc), LOGPIXELSX).max(1) as f32, GetDeviceCaps(Some(dc), LOGPIXELSY).max(1) as f32);
        for (n, p) in pages.iter().enumerate() {
            if stop.load(Ordering::Acquire) {
                let _ = AbortDoc(dc);
                return Err("stopped".into());
            }
            if StartPage(dc) <= 0 {
                let _ = AbortDoc(dc);
                return Err("The printer stopped taking pages.".into());
            }
            let r = page(dc, doc, *p, paper, dpi, stop);
            if EndPage(dc) <= 0 || r.is_err() {
                let _ = AbortDoc(dc);
                return Err(r.err().unwrap_or_else(|| "The printer stopped taking pages.".into()));
            }
            done(n + 1);
        }
        if EndDoc(dc) <= 0 {
            return Err("The printer didn't finish the document.".into());
        }
    }
    Ok(pages.len())
}

/// Draws one page fitted into the printable area (`paper`, device pixels at `dpi`), centred.
fn page(dc: HDC, doc: &Doc, p: Shown, paper: (i32, i32), dpi: (f32, f32), stop: &AtomicBool) -> Result<(), String> {
    let bounds = doc.bounds(p.page)?;
    let list = doc.list(p.page, 1)?;
    let (w, h) = (bounds[2] - bounds[0], bounds[3] - bounds[1]);
    let mut rot = p.rot.rem_euclid(360);
    let sideways = |rot: i32| rot % 180 != 0;
    let (mut pw, mut ph) = if sideways(rot) { (h, w) } else { (w, h) };
    // Turned to suit the paper: back upright when the page was only turned sideways to be read, else a quarter
    // turn to the left (the page's top at the paper's left edge, as printers do).
    if (pw > ph) != (paper.0 > paper.1) && (pw - ph).abs() > 1.0 {
        rot = if (rot + 90) % 360 == 0 { 0 } else { (rot + 270) % 360 };
        (pw, ph) = (ph, pw);
    }
    // Device pixels per point, the same across and down in inches.
    let fit = (paper.0 as f32 / (pw / 72.0 * dpi.0)).min(paper.1 as f32 / (ph / 72.0 * dpi.1));
    let (dw, dh) = ((pw / 72.0 * dpi.0 * fit).round() as i32, (ph / 72.0 * dpi.1 * fit).round() as i32);
    let (dx, dy) = ((paper.0 - dw) / 2, (paper.1 - dh) / 2);
    // Drawn at the printer's resolution up to MAX_DPI; the printer scales the rest.
    let scale = (dpi.0.max(dpi.1) * fit).min(MAX_DPI) / 72.0;
    let (rw, rh) = render_size(&bounds, scale, rot);
    let (rw, rh) = (rw as i32, rh as i32);
    if rw <= 0 || rh <= 0 || dw <= 0 || dh <= 0 {
        return Ok(());
    }
    let mut buf = vec![0u8; (rw * BAND * 4) as usize];
    let mut y = 0;
    while y < rh {
        if stop.load(Ordering::Acquire) {
            return Err("stopped".into());
        }
        let bh = BAND.min(rh - y);
        let out = &mut buf[..(rw * bh * 4) as usize];
        match list.render(&bounds, scale, rot, [0, y, rw, y + bh], out, None)? {
            RenderOutcome::Done => {}
            RenderOutcome::Aborted => return Err("stopped".into()),
        }
        // RGBA to the BGRA Windows wants.
        for px in out.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: rw,
                biHeight: -bh, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        // This band's place on the paper (rounded the same way on each side so bands meet).
        let top = dy + (y as i64 * dh as i64 / rh as i64) as i32;
        let bottom = dy + ((y + bh) as i64 * dh as i64 / rh as i64) as i32;
        // SAFETY: `out` holds rw × bh pixels as described by bmi.
        let drawn = unsafe { StretchDIBits(dc, dx, top, dw, bottom - top, 0, 0, rw, bh, Some(out.as_ptr().cast()), &bmi, DIB_RGB_COLORS, SRCCOPY) };
        if drawn == 0 {
            return Err("The printer couldn't take the page's picture.".into());
        }
        y += bh;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::core::w;
    use windows::Win32::Graphics::Gdi::CreateDCW;

    /// Prints through Windows' own "Microsoft Print to PDF" into a file, no dialog: cargo test --lib print::
    #[test]
    fn prints_to_pdf() {
        let engine = crate::engine::Engine::new(2, 64 << 20).unwrap();
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/../testfiles/text-1000.pdf");
        let info = engine.open(src).unwrap();
        let doc = engine.doc(info.doc_id).unwrap();
        // SAFETY: a printer DC, deleted below.
        let dc = unsafe { CreateDCW(w!("WINSPOOL"), w!("Microsoft Print to PDF"), PCWSTR::null(), None) };
        if dc.is_invalid() {
            println!("no Microsoft Print to PDF here");
            return;
        }
        let out = std::env::temp_dir().join("paginaut-print-test.pdf");
        let _ = std::fs::remove_file(&out);
        let pages = [Shown { page: 0, rot: 0 }, Shown { page: 2, rot: 90 }, Shown { page: 1, rot: 270 }];
        let mut seen = Vec::new();
        let t0 = std::time::Instant::now();
        let r = print_pages(dc, &doc, "test", Some(&out.to_string_lossy()), &pages, &AtomicBool::new(false), |n| seen.push(n));
        unsafe {
            let _ = DeleteDC(dc);
        }
        assert_eq!(r, Ok(3));
        assert_eq!(seen, [1, 2, 3]);
        println!("printed in {:?}", t0.elapsed());
        let printed = engine.open(&out.to_string_lossy()).unwrap();
        assert_eq!(printed.page_count, 3);
        let sizes = engine.page_sizes(printed.doc_id).unwrap();
        println!("paper {sizes:?}");
        let d = engine.doc(printed.doc_id).unwrap();
        for i in 0..3 {
            let [w, h] = sizes[i];
            std::fs::write(std::env::temp_dir().join(format!("paginaut-print-{i}.png")), d.area_data(i, [0.0, 0.0, w, h], 0, "png", 50.0).unwrap()).unwrap();
        }
    }
}
