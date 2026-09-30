//! Microsoft Office as a converter: Word, Excel and PowerPoint driven through COM, by name (late binding),
//! as scripts do. The typed interfaces aren't used: their registration is often broken by Office's
//! click-to-run installs, while late binding keeps working.
//!
//! Files open read-only, macros off, with no prompts: a password-protected file fails instead of asking
//! (a placeholder password is passed; Office ignores it for files without one). Each conversion runs on
//! its own COM thread and gives up after a while if Office stops answering.

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

fn convert_on_this_thread(app: App, input: &str, output: &str) -> Result<(), String> {
    let clsid = unsafe { CLSIDFromProgID(&HSTRING::from(app.prog_id())) }.map_err(|_| format!("{} isn't installed.", app.name()))?;
    let office = Obj(unsafe { CoCreateInstance(&clsid, None, CLSCTX_LOCAL_SERVER) }.map_err(|e| format!("Couldn't start {}: {}", app.name(), e.message()))?);
    // PowerPoint (one window for everything) may already be open for the user: leave it running then.
    let (docs, visible) = match app {
        App::Word => ("Documents", office.get("Visible").ok().and_then(|v| v.int()).unwrap_or(0) != 0),
        App::Excel => ("Workbooks", office.get("Visible").ok().and_then(|v| v.int()).unwrap_or(0) != 0),
        App::PowerPoint => ("Presentations", office.get("Visible").ok().and_then(|v| v.int()).unwrap_or(0) != 0),
    };
    let was_in_use = visible || office.count(docs) > 0;
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
                // OutputFileName, ExportFormat (PDF), OpenAfterExport, OptimizeFor (print), Range (all), From, To, Item (content),
                // IncludeDocProps, KeepIRM, CreateBookmarks (from headings), DocStructureTags, BitmapMissingFonts
                let r = doc.call(
                    "ExportAsFixedFormat",
                    &[
                        Arg::Str(output), Arg::Int(17), Arg::Bool(false), Arg::Int(0), Arg::Int(0), Arg::Int(1), Arg::Int(1), Arg::Int(0),
                        Arg::Bool(true), Arg::Bool(true), Arg::Int(1), Arg::Bool(true), Arg::Bool(true),
                    ],
                );
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
    result
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
    // Office reads relative paths from its own folder.
    let input = std::path::absolute(input).map_err(|e| e.to_string())?.to_string_lossy().into_owned();
    // Click-to-run Office doesn't see folders made in the local app data folder after it was set up ("The
    // directory name isn't valid"), which is where Paginaut keeps its PDFs; the temp folder it does see.
    let n = RUN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temp = std::env::temp_dir().join(format!("paginaut-office-{}-{n}.pdf", std::process::id()));
    let _ = std::fs::remove_file(&temp);
    let r = run(app, &input, &temp.to_string_lossy(), timeout).and_then(|()| {
        std::fs::rename(&temp, output).or_else(|_| std::fs::copy(&temp, output).map(drop)).map_err(|e| format!("Couldn't keep the PDF: {e}"))
    });
    let _ = std::fs::remove_file(&temp);
    r
}

fn run(app: App, input: &str, output: &str, timeout: Duration) -> Result<(), String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let (input, output) = (input.to_string(), output.to_string());
    std::thread::Builder::new()
        .name("office-com".into())
        .spawn(move || {
            let init = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            let r = if init.is_ok() { convert_on_this_thread(app, &input, &output) } else { Err(format!("COM: {}", init.message())) };
            if init.is_ok() {
                unsafe { CoUninitialize() };
            }
            let _ = tx.send(r);
        })
        .map_err(|e| e.to_string())?;
    match rx.recv_timeout(timeout) {
        Ok(r) => r,
        Err(_) => Err(format!("{} stopped answering. It may be showing a message: check for its window.", app.name())),
    }
}
