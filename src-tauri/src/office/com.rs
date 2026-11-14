//! Microsoft Office as a converter: Word, Excel and PowerPoint driven through COM, by name (late binding),
//! as scripts do. The typed interfaces aren't used: their registration is often broken by Office's
//! click-to-run installs, while late binding keeps working.
//!
//! Files open read-only, macros off, with no prompts: a password-protected file fails instead of asking
//! (a placeholder password is passed; Office ignores it for files without one). Each conversion runs on
//! its own COM thread and gives up after a while if Office stops answering.

/// Which Office program converts a file.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum App {
    Word,
    Excel,
    PowerPoint,
}

impl App {
    pub fn prog_id(self) -> &'static str {
        match self {
            App::Word => "Word.Application",
            App::Excel => "Excel.Application",
            App::PowerPoint => "PowerPoint.Application",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            App::Word => "Microsoft Word",
            App::Excel => "Microsoft Excel",
            App::PowerPoint => "Microsoft PowerPoint",
        }
    }
}

#[cfg(windows)]
pub use win::*;

#[cfg(windows)]
mod win {
    use super::App;
    use std::mem::ManuallyDrop;
    use std::time::Duration;
    use windows::core::{BSTR, GUID, HSTRING, PCWSTR};
    use windows::Win32::Foundation::{DISP_E_EXCEPTION, DISP_E_PARAMNOTFOUND, RPC_E_CALL_REJECTED, RPC_E_SERVERCALL_RETRYLATER, VARIANT_BOOL};
    use windows::Win32::System::Com::{
        CLSIDFromProgID, CoCreateInstance, CoInitializeEx, CoUninitialize, IDispatch, CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED, DISPATCH_FLAGS,
        DISPATCH_METHOD, DISPATCH_PROPERTYGET, DISPATCH_PROPERTYPUT, DISPPARAMS, EXCEPINFO,
    };
    use windows::Win32::System::Ole::DISPID_PROPERTYPUT;
    use windows::Win32::System::Variant::{VariantClear, VARIANT, VT_BOOL, VT_BSTR, VT_DISPATCH, VT_ERROR, VT_I4};

    /// Whether the program is registered (installed).
    pub fn installed(app: App) -> bool {
        unsafe { CLSIDFromProgID(&HSTRING::from(app.prog_id())).is_ok() }
    }

    enum Arg<'a> {
        Str(&'a str),
        Bool(bool),
        Int(i32),
        /// An optional argument left out.
        Missing,
    }

    /// A VARIANT that frees what it holds.
    struct Var(VARIANT);

    impl Drop for Var {
        fn drop(&mut self) {
            unsafe {
                let _ = VariantClear(&mut self.0);
            }
        }
    }

    impl Var {
        fn of(a: &Arg) -> Var {
            let mut v = VARIANT::default();
            unsafe {
                let inner = &mut *v.Anonymous.Anonymous;
                match a {
                    Arg::Str(s) => {
                        inner.vt = VT_BSTR;
                        inner.Anonymous.bstrVal = ManuallyDrop::new(BSTR::from(*s));
                    }
                    Arg::Bool(b) => {
                        inner.vt = VT_BOOL;
                        inner.Anonymous.boolVal = VARIANT_BOOL(if *b { -1 } else { 0 });
                    }
                    Arg::Int(i) => {
                        inner.vt = VT_I4;
                        inner.Anonymous.lVal = *i;
                    }
                    Arg::Missing => {
                        inner.vt = VT_ERROR;
                        inner.Anonymous.scode = DISP_E_PARAMNOTFOUND.0;
                    }
                }
            }
            Var(v)
        }

        fn object(&self) -> Result<Obj, String> {
            unsafe {
                let inner = &*self.0.Anonymous.Anonymous;
                if inner.vt == VT_DISPATCH {
                    if let Some(d) = (*inner.Anonymous.pdispVal).clone() {
                        return Ok(Obj(d));
                    }
                }
            }
            Err("Office returned something unexpected".into())
        }

        fn int(&self) -> Option<i32> {
            unsafe {
                let inner = &*self.0.Anonymous.Anonymous;
                match inner.vt {
                    VT_I4 => Some(inner.Anonymous.lVal),
                    VT_BOOL => Some(inner.Anonymous.boolVal.0 as i32),
                    _ => None,
                }
            }
        }
    }

    /// An Office object, used by member names.
    struct Obj(IDispatch);

    impl Obj {
        fn invoke(&self, name: &str, flags: DISPATCH_FLAGS, args: &[Arg]) -> Result<Var, String> {
            let wide = HSTRING::from(name);
            let names = [PCWSTR(wide.as_ptr())];
            let mut id = 0i32;
            unsafe { self.0.GetIDsOfNames(&GUID::zeroed(), names.as_ptr(), 1, 0x0400, &mut id) }.map_err(|e| format!("{name}: {}", e.message()))?;
            // Arguments go in reverse order.
            let mut vars: Vec<Var> = args.iter().rev().map(Var::of).collect();
            let mut raw: Vec<VARIANT> = vars.iter().map(|v| unsafe { std::ptr::read(&v.0) }).collect();
            let mut put = DISPID_PROPERTYPUT;
            let is_put = flags == DISPATCH_PROPERTYPUT;
            let params = DISPPARAMS {
                rgvarg: raw.as_mut_ptr(),
                rgdispidNamedArgs: if is_put { &mut put } else { std::ptr::null_mut() },
                cArgs: raw.len() as u32,
                cNamedArgs: is_put as u32,
            };
            let mut tries = 0;
            let result = loop {
                let mut result = Var(VARIANT::default());
                let mut info = EXCEPINFO::default();
                let r = unsafe { self.0.Invoke(id, &GUID::zeroed(), 0x0400, flags, &params, Some(&mut result.0), Some(&mut info), None) };
                let described = unsafe {
                    let d = ManuallyDrop::take(&mut info.bstrDescription).to_string();
                    drop(ManuallyDrop::take(&mut info.bstrSource));
                    drop(ManuallyDrop::take(&mut info.bstrHelpFile));
                    d
                };
                match r {
                    Ok(()) => break Ok(result),
                    // Office is busy (starting up, or in the middle of something): ask again shortly.
                    Err(e) if (e.code() == RPC_E_CALL_REJECTED || e.code() == RPC_E_SERVERCALL_RETRYLATER) && tries < 100 => {
                        tries += 1;
                        std::thread::sleep(Duration::from_millis(100));
                    }
                    Err(e) if e.code() == DISP_E_EXCEPTION && !described.is_empty() => break Err(described.trim().to_string()),
                    Err(e) => break Err(format!("{name}: {}", e.message())),
                }
            };
            // The VARIANTs were copied bitwise into `raw`; `vars` frees them once.
            std::mem::forget(raw);
            drop(vars.drain(..));
            result
        }

        fn get(&self, name: &str) -> Result<Var, String> {
            self.invoke(name, DISPATCH_PROPERTYGET, &[])
        }
        fn put(&self, name: &str, value: Arg) -> Result<(), String> {
            self.invoke(name, DISPATCH_PROPERTYPUT, &[value]).map(drop)
        }
        fn call(&self, name: &str, args: &[Arg]) -> Result<Var, String> {
            self.invoke(name, DISPATCH_METHOD | DISPATCH_PROPERTYGET, args)
        }
        fn count(&self, collection: &str) -> i32 {
            self.get(collection).and_then(|c| c.object()).and_then(|c| c.get("Count")).ok().and_then(|v| v.int()).unwrap_or(0)
        }
    }

    /// A password Office tries on protected files instead of asking; files without one ignore it.
    const NO_PASSWORD: &str = "\u{1}paginaut";

    /// What Office is asked to do.
    #[derive(Clone)]
    enum Job {
        /// The file as a PDF.
        ToPdf(App),
        /// A PDF as an editable Word document (Word's own PDF reading).
        PdfToWord,
        /// A presentation with a slide per picture: (picture file, width, height in points).
        Slides(Vec<(String, f32, f32)>),
    }

    impl Job {
        fn app(&self) -> App {
            match self {
                Job::ToPdf(a) => *a,
                Job::PdfToWord => App::Word,
                Job::Slides(_) => App::PowerPoint,
            }
        }
    }

    /// The running copies of a program, by process id.
    fn running(exe: &str) -> std::collections::HashSet<u32> {
        use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
        let mut sys = System::new();
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
        sys.processes().iter().filter(|(_, p)| p.name().eq_ignore_ascii_case(exe)).map(|(pid, _)| pid.as_u32()).collect()
    }

    /// `started`: the process this conversion started for itself (none when it joined a copy the user has open),
    /// while it's at work. If it stops answering, `run` ends that process: it has no window to close it by, and
    /// would keep the file locked.
    fn convert_on_this_thread(job: &Job, input: &str, output: &str, started: &std::sync::Mutex<Option<u32>>) -> Result<(), String> {
        let app = job.app();
        let clsid = unsafe { CLSIDFromProgID(&HSTRING::from(app.prog_id())) }.map_err(|_| format!("{} isn't installed.", app.name()))?;
        let exe = match app {
            App::Word => "WINWORD.EXE",
            App::Excel => "EXCEL.EXE",
            App::PowerPoint => "POWERPNT.EXE",
        };
        let before = running(exe);
        let office = Obj(unsafe { CoCreateInstance(&clsid, None, CLSCTX_LOCAL_SERVER) }.map_err(|e| format!("Couldn't start {}: {}", app.name(), e.message()))?);
        // PowerPoint (one window for everything) may already be open for the user: leave it running then.
        let docs = match app {
            App::Word => "Documents",
            App::Excel => "Workbooks",
            App::PowerPoint => "Presentations",
        };
        let visible = office.get("Visible").ok().and_then(|v| v.int()).unwrap_or(0) != 0;
        let was_in_use = visible || office.count(docs) > 0;
        if !was_in_use {
            // Exactly one new process of that program since just before: the one made for this.
            let new: Vec<u32> = running(exe).difference(&before).copied().collect();
            if let [pid] = new[..] {
                *started.lock().unwrap() = Some(pid);
            }
        }
        let _ = office.put("AutomationSecurity", Arg::Int(3)); // msoAutomationSecurityForceDisable: no macros
        let result = (|| -> Result<(), String> {
            match app {
                App::Word => {
                    if !was_in_use {
                        let _ = office.put("Visible", Arg::Bool(false));
                    }
                    let _ = office.put("DisplayAlerts", Arg::Int(0));
                    let documents = office.get("Documents")?.object()?;
                    // FileName, ConfirmConversions, ReadOnly, AddToRecentFiles, PasswordDocument, PasswordTemplate, Revert,
                    // WritePasswordDocument, WritePasswordTemplate, Format, Encoding, Visible, OpenAndRepair, DocumentDirection,
                    // NoEncodingDialog
                    let doc = documents
                        .call(
                            "Open",
                            &[
                                Arg::Str(input), Arg::Bool(false), Arg::Bool(true), Arg::Bool(false), Arg::Str(NO_PASSWORD), Arg::Str(NO_PASSWORD), Arg::Bool(false),
                                Arg::Missing, Arg::Missing, Arg::Missing, Arg::Missing, Arg::Bool(false), Arg::Missing, Arg::Missing, Arg::Bool(true),
                            ],
                        )
                        .map_err(protected)?
                        .object()?;
                    let r = if matches!(job, Job::PdfToWord) {
                        doc.call("SaveAs2", &[Arg::Str(output), Arg::Int(16)]) // wdFormatXMLDocument (.docx)
                    } else {
                        // OutputFileName, ExportFormat (PDF), OpenAfterExport, OptimizeFor (print), Range (all), From, To, Item
                        // (content), IncludeDocProps, KeepIRM, CreateBookmarks (from headings), DocStructureTags, BitmapMissingFonts
                        doc.call(
                            "ExportAsFixedFormat",
                            &[
                                Arg::Str(output), Arg::Int(17), Arg::Bool(false), Arg::Int(0), Arg::Int(0), Arg::Int(1), Arg::Int(1), Arg::Int(0),
                                Arg::Bool(true), Arg::Bool(true), Arg::Int(1), Arg::Bool(true), Arg::Bool(true),
                            ],
                        )
                    };
                    let _ = doc.call("Close", &[Arg::Int(0)]); // wdDoNotSaveChanges
                    r.map(drop)
                }
                App::Excel => {
                    let _ = office.put("DisplayAlerts", Arg::Bool(false));
                    let _ = office.put("AskToUpdateLinks", Arg::Bool(false));
                    let _ = office.put("EnableEvents", Arg::Bool(false));
                    let books = office.get("Workbooks")?.object()?;
                    // FileName, UpdateLinks (never), ReadOnly, Format, Password, WriteResPassword, IgnoreReadOnlyRecommended,
                    // Origin, Delimiter, Editable, Notify, Converter, AddToMru
                    let book = books
                        .call(
                            "Open",
                            &[
                                Arg::Str(input), Arg::Int(0), Arg::Bool(true), Arg::Missing, Arg::Str(NO_PASSWORD), Arg::Str(NO_PASSWORD), Arg::Bool(true),
                                Arg::Missing, Arg::Missing, Arg::Bool(false), Arg::Bool(false), Arg::Missing, Arg::Bool(false),
                            ],
                        )
                        .map_err(protected)?
                        .object()?;
                    // Type (PDF), Filename, Quality (standard), IncludeDocProperties, IgnorePrintAreas
                    let r = book.call("ExportAsFixedFormat", &[Arg::Int(0), Arg::Str(output), Arg::Int(0), Arg::Bool(true), Arg::Bool(false)]);
                    let _ = book.call("Close", &[Arg::Bool(false)]);
                    r.map(drop).map_err(|e| if e.contains("nothing to print") || e.contains("Document not saved") { "The workbook has nothing to print.".into() } else { e })
                }
                App::PowerPoint => {
                    let _ = office.put("DisplayAlerts", Arg::Int(1)); // ppAlertsNone
                    let shows = office.get("Presentations")?.object()?;
                    if let Job::Slides(pictures) = job {
                        return slides(&shows, pictures, output);
                    }
                    // "file::password" opens a protected presentation without asking; files without one ignore it.
                    // FileName, ReadOnly, Untitled, WithWindow (all msoTriState)
                    let named = format!("{input}::{NO_PASSWORD}");
                    let show = shows.call("Open", &[Arg::Str(&named), Arg::Int(-1), Arg::Int(0), Arg::Int(0)]).map_err(protected)?.object()?;
                    let r = show.call("SaveAs", &[Arg::Str(output), Arg::Int(32)]); // ppSaveAsPDF
                    let _ = show.call("Close", &[]);
                    r.map(drop)
                }
            }
        })();
        if !was_in_use && office.count(docs) == 0 {
            let _ = match app {
                App::Word => office.call("Quit", &[Arg::Int(0)]),
                _ => office.call("Quit", &[]),
            };
        }
        // Answering again (or done): no longer `run`'s to end.
        *started.lock().unwrap() = None;
        result
    }

    /// A presentation (no window) with a slide per picture, each filling its slide; the slides take the first
    /// picture's shape (PowerPoint allows 1 to 56 inches a side).
    fn slides(shows: &Obj, pictures: &[(String, f32, f32)], output: &str) -> Result<(), String> {
        let show = shows.call("Add", &[Arg::Int(0)])?.object()?;
        let r = (|| -> Result<(), String> {
            let (w0, h0) = pictures.first().map_or((720.0, 540.0), |p| (p.1, p.2));
            let fit = (4032.0 / w0.max(h0)).min(1.0);
            let (sw, sh) = ((w0 * fit).max(72.0), (h0 * fit).max(72.0));
            let setup = show.get("PageSetup")?.object()?;
            setup.put("SlideWidth", Arg::Int(sw.round() as i32))?;
            setup.put("SlideHeight", Arg::Int(sh.round() as i32))?;
            let list = show.get("Slides")?.object()?;
            for (i, (file, w, h)) in pictures.iter().enumerate() {
                let slide = list.call("Add", &[Arg::Int(i as i32 + 1), Arg::Int(12)])?.object()?; // ppLayoutBlank
                // A page of another shape fits inside, centred.
                let z = (sw / w).min(sh / h);
                let (pw, ph) = (w * z, h * z);
                let shapes = slide.get("Shapes")?.object()?;
                // FileName, LinkToFile, SaveWithDocument, Left, Top, Width, Height
                shapes.call(
                    "AddPicture",
                    &[
                        Arg::Str(file), Arg::Int(0), Arg::Int(-1), Arg::Int(((sw - pw) / 2.0) as i32), Arg::Int(((sh - ph) / 2.0) as i32), Arg::Int(pw.round() as i32),
                        Arg::Int(ph.round() as i32),
                    ],
                )?;
            }
            show.call("SaveAs", &[Arg::Str(output), Arg::Int(24)]).map(drop) // ppSaveAsOpenXMLPresentation
        })();
        let _ = show.call("Close", &[]);
        r
    }

    /// Office's message for a wrong password, in words the user can act on.
    fn protected(e: String) -> String {
        let l = e.to_lowercase();
        if l.contains("password") {
            "The file is password-protected. Open it in Office, remove the password (or save a copy without one), and try again.".into()
        } else {
            e
        }
    }

    static RUN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    /// Converts `input` to the PDF `output` with `app`, giving up after `timeout`.
    pub fn convert(app: App, input: &str, output: &str, timeout: Duration) -> Result<(), String> {
        job(Job::ToPdf(app), input, output, "pdf", timeout)
    }

    /// A PDF as an editable Word document, made by Word itself.
    pub fn pdf_to_word(input: &str, output: &str, timeout: Duration) -> Result<(), String> {
        job(Job::PdfToWord, input, output, "docx", timeout)
    }

    /// A PowerPoint presentation with a slide per picture (file, width and height in points).
    pub fn picture_slides(pictures: Vec<(String, f32, f32)>, output: &str, timeout: Duration) -> Result<(), String> {
        job(Job::Slides(pictures), "", output, "pptx", timeout)
    }

    fn job(job: Job, input: &str, output: &str, ext: &str, timeout: Duration) -> Result<(), String> {
        // Office reads relative paths from its own folder.
        let input = if input.is_empty() { String::new() } else { std::path::absolute(input).map_err(|e| e.to_string())?.to_string_lossy().into_owned() };
        // Click-to-run Office doesn't see folders made in the local app data folder after it was set up ("The
        // directory name isn't valid"), which is where Paginaut keeps its PDFs; the temp folder it does see.
        let n = RUN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let temp = std::env::temp_dir().join(format!("paginaut-office-{}-{n}.{ext}", std::process::id()));
        let _ = std::fs::remove_file(&temp);
        let r = run(job, &input, &temp.to_string_lossy(), timeout).and_then(|()| {
            std::fs::rename(&temp, output).or_else(|_| std::fs::copy(&temp, output).map(drop)).map_err(|e| format!("Couldn't keep the file: {e}"))
        });
        let _ = std::fs::remove_file(&temp);
        r
    }

    fn run(job: Job, input: &str, output: &str, timeout: Duration) -> Result<(), String> {
        let app = job.app();
        let (tx, rx) = std::sync::mpsc::channel();
        let (input, output) = (input.to_string(), output.to_string());
        let started = std::sync::Arc::new(std::sync::Mutex::new(None::<u32>));
        let theirs = started.clone();
        std::thread::Builder::new()
            .name("office-com".into())
            .spawn(move || {
                let init = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
                let r = if init.is_ok() { convert_on_this_thread(&job, &input, &output, &theirs) } else { Err(format!("COM: {}", init.message())) };
                if init.is_ok() {
                    unsafe { CoUninitialize() };
                }
                let _ = tx.send(r);
            })
            .map_err(|e| e.to_string())?;
        match rx.recv_timeout(timeout) {
            Ok(r) => r,
            Err(_) => {
                // The copy started for this conversion is hidden, with the file open: nothing the user could close.
                // It's ended here, so the file isn't kept locked. A copy the user had open is theirs and is left.
                let ended = started.lock().unwrap().take().is_some_and(|pid| {
                    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
                    let mut sys = System::new();
                    sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), true, ProcessRefreshKind::nothing());
                    sys.process(Pid::from_u32(pid)).is_some_and(|p| p.kill())
                });
                Err(if ended {
                    format!("{} stopped answering, so Paginaut closed the hidden copy it had started. The file may be damaged, protected, or need something {0} asks about when it's opened there.", app.name())
                } else {
                    format!("{} stopped answering. It may be showing a message: check for its window.", app.name())
                })
            }
        }
    }
}

/// Without Windows there's no Microsoft Office to drive: LibreOffice converts (libre.rs).
#[cfg(not(windows))]
pub use other::*;

#[cfg(not(windows))]
mod other {
    use super::App;
    use std::time::Duration;

    const ONLY_WINDOWS: &str = "Microsoft Office is only on Windows.";

    pub fn installed(_: App) -> bool {
        false
    }
    pub fn convert(_: App, _: &str, _: &str, _: Duration) -> Result<(), String> {
        Err(ONLY_WINDOWS.into())
    }
    pub fn pdf_to_word(_: &str, _: &str, _: Duration) -> Result<(), String> {
        Err(ONLY_WINDOWS.into())
    }
    pub fn picture_slides(_: Vec<(String, f32, f32)>, _: &str, _: Duration) -> Result<(), String> {
        Err(ONLY_WINDOWS.into())
    }
}
