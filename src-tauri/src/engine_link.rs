//! Android: the engine (MuPDF, which reads every document) runs in its own isolated process, EngineService:
//! no permissions, no files, no network. The app opens each document itself, read-only, and passes the engine
//! the open file; the engine sends back pages and text. A document built to break the engine lands somewhere
//! with nothing to reach.
//!
//! The two processes talk over a socket pair (`Link`): each message is a JSON head and optional bytes (page
//! pixels, a cover), with file handles passed alongside. The app's side is `Remote`, the engine's `Host`.
//!
//! If the engine stops (a damaged or hostile file, or Android reclaiming memory), `Remote` starts another and
//! opens the documents again as they were, repeating what was asked: nobody has to restart the app. A document
//! that stops the engine twice within two minutes is refused, so a bad file can't keep it stopping.

use crate::app::{self, ReflowEvent, Res, SearchEvent};
use crate::engine::{sandbox, DocId, Engine, RenderReq, SearchOptions};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::io;
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, OnceLock, Weak};
use std::time::{Duration, Instant};
use tauri::ipc::Channel;

/// What a call gets when the engine stopped while it waited (`Remote` then tries again on a new one).
pub const STOPPED: &str = "Paginaut's engine stopped.";

/// Temporary files one engine may ask for (a solid comic book takes one per open copy; reading uses a few).
const MAX_TEMP_FILES: u64 = 512;

/// Space a runaway engine may take before it's stopped (see `Remote::watch`).
const MAX_ENGINE_GROWTH: u64 = 512 << 20;

/// Said for a document that stopped the engine twice.
const REFUSED: &str = "This document stopped Paginaut's reading engine twice, so it was closed. The file may be damaged.";

// ---------------------------------------------------------------- messages

/// One message: a JSON head, bytes after it, and file handles passed with it.
pub struct Frame {
    pub head: Value,
    pub bin: Vec<u8>,
    pub fds: Vec<OwnedFd>,
}

/// An answer: a value, bytes and handles.
pub struct Reply {
    pub v: Value,
    pub bin: Vec<u8>,
    pub fds: Vec<OwnedFd>,
}

impl Reply {
    fn value(v: Value) -> Reply {
        Reply { v, bin: Vec::new(), fds: Vec::new() }
    }
}

const MAX_FDS: usize = 4;
const MAX_HEAD: usize = 64 << 20;
const MAX_BYTES: usize = 256 << 20;

/// Writes all of `buf` (MSG_NOSIGNAL: a closed socket is an error, not a signal that ends the process).
fn send_all(fd: RawFd, mut buf: &[u8]) -> io::Result<()> {
    while !buf.is_empty() {
        let n = unsafe { libc::send(fd, buf.as_ptr().cast(), buf.len(), libc::MSG_NOSIGNAL) };
        if n < 0 {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(e);
        }
        buf = &buf[n as usize..];
    }
    Ok(())
}

/// Head length and bytes length (u32 each), the head, then the bytes; handles go with the first part.
fn send_frame(sock: &UnixStream, head: &Value, bin: &[u8], fds: &[RawFd]) -> io::Result<()> {
    let json = serde_json::to_vec(head).map_err(io::Error::other)?;
    let mut first = Vec::with_capacity(8 + json.len());
    first.extend_from_slice(&(json.len() as u32).to_le_bytes());
    first.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    first.extend_from_slice(&json);
    let fd = sock.as_raw_fd();
    let mut sent = 0;
    if !fds.is_empty() {
        let fds = &fds[..fds.len().min(MAX_FDS)];
        let bytes = std::mem::size_of_val(fds);
        let space = unsafe { libc::CMSG_SPACE(bytes as u32) } as usize;
        let mut control = vec![0u8; space];
        let mut iov = libc::iovec { iov_base: first.as_ptr() as *mut _, iov_len: first.len() };
        // SAFETY: msghdr is plain data; the buffers it points to outlive the call.
        let n = unsafe {
            let mut msg: libc::msghdr = std::mem::zeroed();
            msg.msg_iov = &mut iov;
            msg.msg_iovlen = 1;
            msg.msg_control = control.as_mut_ptr().cast();
            msg.msg_controllen = space as _;
            let c = libc::CMSG_FIRSTHDR(&msg);
            (*c).cmsg_level = libc::SOL_SOCKET;
            (*c).cmsg_type = libc::SCM_RIGHTS;
            (*c).cmsg_len = libc::CMSG_LEN(bytes as u32) as _;
            std::ptr::copy_nonoverlapping(fds.as_ptr(), libc::CMSG_DATA(c).cast::<RawFd>(), fds.len());
            loop {
                let n = libc::sendmsg(fd, &msg, libc::MSG_NOSIGNAL);
                if n < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                break n;
            }
        };
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        sent = n as usize;
    }
    send_all(fd, &first[sent..])?;
    send_all(fd, bin)
}

/// Reads exactly `buf.len()` bytes, keeping any handles that come with them.
fn recv_exact(fd: RawFd, buf: &mut [u8], fds: &mut Vec<OwnedFd>) -> io::Result<()> {
    let mut got = 0;
    let space = unsafe { libc::CMSG_SPACE((MAX_FDS * std::mem::size_of::<RawFd>()) as u32) } as usize;
    let mut control = vec![0u8; space];
    while got < buf.len() {
        let mut iov = libc::iovec { iov_base: buf[got..].as_mut_ptr().cast(), iov_len: buf.len() - got };
        // SAFETY: as in send_frame; handles received are owned from here on.
        let n = unsafe {
            let mut msg: libc::msghdr = std::mem::zeroed();
            msg.msg_iov = &mut iov;
            msg.msg_iovlen = 1;
            msg.msg_control = control.as_mut_ptr().cast();
            msg.msg_controllen = space as _;
            let n = libc::recvmsg(fd, &mut msg, libc::MSG_CMSG_CLOEXEC);
            if n > 0 {
                let mut c = libc::CMSG_FIRSTHDR(&msg);
                while !c.is_null() {
                    if (*c).cmsg_level == libc::SOL_SOCKET && (*c).cmsg_type == libc::SCM_RIGHTS {
                        let len = (*c).cmsg_len as usize - libc::CMSG_LEN(0) as usize;
                        let data = libc::CMSG_DATA(c).cast::<RawFd>();
                        for i in 0..len / std::mem::size_of::<RawFd>() {
                            fds.push(OwnedFd::from_raw_fd(*data.add(i)));
                        }
                    }
                    c = libc::CMSG_NXTHDR(&msg, c);
                }
            }
            n
        };
        if n < 0 {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(e);
        }
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        got += n as usize;
    }
    Ok(())
}

fn recv_frame(sock: &UnixStream) -> io::Result<Frame> {
    let fd = sock.as_raw_fd();
    let mut fds = Vec::new();
    let mut lens = [0u8; 8];
    recv_exact(fd, &mut lens, &mut fds)?;
    let json_len = u32::from_le_bytes(lens[..4].try_into().unwrap()) as usize;
    let bin_len = u32::from_le_bytes(lens[4..].try_into().unwrap()) as usize;
    // Bounds for what the other side may claim (a page's pixels are at most a few tens of MB): an engine
    // taken over by a hostile file can't make the app allocate gigabytes and stop.
    if json_len > MAX_HEAD || bin_len > MAX_BYTES {
        return Err(io::Error::other("message too long"));
    }
    let mut json = vec![0u8; json_len];
    recv_exact(fd, &mut json, &mut fds)?;
    let mut bin = vec![0u8; bin_len];
    recv_exact(fd, &mut bin, &mut fds)?;
    let head = serde_json::from_slice(&json).map_err(io::Error::other)?;
    Ok(Frame { head, bin, fds })
}

// ---------------------------------------------------------------- the link

/// One side of the socket: calls the other side and waits for its answer; what the other side asks is
/// handed to `on_frame`.
pub struct Link {
    sock: UnixStream,
    write: Mutex<()>,
    next: AtomicU64,
    pending: Mutex<HashMap<u64, mpsc::Sender<Result<Frame, String>>>>,
    alive: AtomicBool,
}

impl Link {
    /// Starts reading `sock`. Requests and events go to `on_frame` (on the reading thread: it must not wait
    /// for an answer itself); `on_end` runs once the other side has gone.
    pub fn start(sock: UnixStream, on_frame: impl Fn(&Arc<Link>, Frame) + Send + 'static, on_end: impl FnOnce() + Send + 'static) -> io::Result<Arc<Link>> {
        let reader = sock.try_clone()?;
        let link = Arc::new(Link { sock, write: Mutex::new(()), next: AtomicU64::new(1), pending: Mutex::new(HashMap::new()), alive: AtomicBool::new(true) });
        let l = link.clone();
        std::thread::Builder::new().name("engine-link".into()).spawn(move || {
            while let Ok(f) = recv_frame(&reader) {
                if f.head["t"] == "res" {
                    let id = f.head["id"].as_u64().unwrap_or(0);
                    if let Some(tx) = l.pending.lock().remove(&id) {
                        let _ = tx.send(Ok(f));
                    }
                } else {
                    on_frame(&l, f);
                }
            }
            l.alive.store(false, Ordering::SeqCst);
            for (_, tx) in l.pending.lock().drain() {
                let _ = tx.send(Err(STOPPED.into()));
            }
            on_end();
        })?;
        Ok(link)
    }

    pub fn alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    fn send(&self, head: &Value, bin: &[u8], fds: &[RawFd]) -> Result<(), String> {
        let _one = self.write.lock();
        send_frame(&self.sock, head, bin, fds).map_err(|_| STOPPED.to_string())
    }

    /// Asks the other side to do `cmd` and waits for the answer (an error if it said so, or STOPPED).
    pub fn call(&self, cmd: &str, args: Value, bin: &[u8], fds: &[RawFd]) -> Result<Frame, String> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        self.pending.lock().insert(id, tx);
        // Checked after joining the waiting list: the reader empties the list only after marking the link dead.
        if !self.alive() {
            self.pending.lock().remove(&id);
            return Err(STOPPED.into());
        }
        if let Err(e) = self.send(&json!({ "t": "req", "id": id, "cmd": cmd, "args": args }), bin, fds) {
            self.pending.lock().remove(&id);
            return Err(e);
        }
        let f = rx.recv().map_err(|_| STOPPED.to_string())??;
        if f.head["ok"] == true {
            Ok(f)
        } else {
            Err(f.head["e"].as_str().unwrap_or("The engine couldn't do that.").to_string())
        }
    }

    pub fn reply(&self, id: u64, res: Result<Reply, String>) {
        let _ = match res {
            Ok(r) => {
                let fds: Vec<RawFd> = r.fds.iter().map(|f| f.as_raw_fd()).collect();
                self.send(&json!({ "t": "res", "id": id, "ok": true, "v": r.v }), &r.bin, &fds)
            }
            Err(e) => self.send(&json!({ "t": "res", "id": id, "ok": false, "e": e }), &[], &[]),
        };
    }

    /// One event of a stream (a search's hits, Read mode's pages).
    pub fn event(&self, sid: u64, v: Value) {
        let _ = self.send(&json!({ "t": "ev", "sid": sid, "v": v }), &[], &[]);
    }

    pub fn close(&self) {
        let _ = self.sock.shutdown(std::net::Shutdown::Both);
    }
}

// ---------------------------------------------------------------- the engine's side

/// The engine of this process (one, whichever connection uses it).
static ENGINE: OnceLock<Arc<Engine>> = OnceLock::new();

/// Connections being served. When the last goes (the app went, or gave up on an engine that stopped
/// answering), the process ends: a stuck engine doesn't stay behind using the battery.
static LIVE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// EngineService (Kotlin) gives the engine one end of the app's socket pair: served until the app goes.
#[no_mangle]
pub extern "C" fn Java_app_paginaut_reader_EngineService_serve(_env: *mut std::ffi::c_void, _this: *mut std::ffi::c_void, fd: std::os::raw::c_int) {
    // SAFETY: the handle was detached from its ParcelFileDescriptor for this.
    let sock = unsafe { UnixStream::from_raw_fd(fd) };
    let engine = match ENGINE.get() {
        Some(e) => e.clone(),
        None => {
            // A reader runs no form scripts.
            crate::engine::mupdf::allow_scripts(false);
            match Engine::new(crate::worker_count(), crate::store_bytes()) {
                Ok(e) => ENGINE.get_or_init(|| e).clone(),
                Err(_) => return,
            }
        }
    };
    let host = Arc::new(Host { engine, files: Mutex::new(HashMap::new()) });
    let (h, ended) = (host.clone(), host.clone());
    LIVE.fetch_add(1, Ordering::SeqCst);
    let gone = || {
        if LIVE.fetch_sub(1, Ordering::SeqCst) == 1 {
            // SAFETY: nothing to save; _exit skips C++ destructors that could meet a worker mid-page.
            unsafe { libc::_exit(0) }
        }
    };
    let link = match Link::start(sock, move |link, f| h.request(link, f), move || {
        ended.end();
        gone();
    }) {
        Ok(l) => l,
        Err(_) => return gone(),
    };
    *CURRENT.lock() = Arc::downgrade(&link);
    sandbox::set_scratch(Box::new(AppFiles));
    // The reading thread does the work; this one (EngineService's) just stays.
    while link.alive() {
        std::thread::park_timeout(Duration::from_secs(60));
    }
}

/// The engine serving one connection: the documents it opened, with the files the app gave for them.
struct Host {
    engine: Arc<Engine>,
    files: Mutex<HashMap<DocId, OwnedFd>>,
}

/// The connection the engine's temporary files are asked for on (the latest).
static CURRENT: Mutex<Weak<Link>> = Mutex::new(Weak::new());

/// The engine's temporary files, made by the app (sandbox::Scratch).
struct AppFiles;

impl AppFiles {
    fn ask(&self, args: Value) -> Result<Frame, String> {
        let link = CURRENT.lock().upgrade().ok_or(STOPPED)?;
        link.call("scratch", args, &[], &[])
    }
}

impl sandbox::Scratch for AppFiles {
    fn anon(&self) -> Result<OwnedFd, String> {
        self.ask(json!({ "op": "anon" }))?.fds.pop().ok_or_else(|| "no file".into())
    }
    fn open(&self, name: &str) -> Option<OwnedFd> {
        self.ask(json!({ "op": "open", "name": name })).ok()?.fds.pop()
    }
    fn create(&self, name: &str) -> Result<OwnedFd, String> {
        self.ask(json!({ "op": "create", "name": name }))?.fds.pop().ok_or_else(|| "no file".into())
    }
    fn commit(&self, name: &str) -> Result<(), String> {
        self.ask(json!({ "op": "commit", "name": name })).map(|_| ())
    }
}

fn arg<T: serde::de::DeserializeOwned>(args: &Value, key: &str) -> Result<T, String> {
    serde_json::from_value(args.get(key).cloned().unwrap_or(Value::Null)).map_err(|e| format!("{key}: {e}"))
}

fn to_value<T: serde::Serialize>(v: T) -> Result<Reply, String> {
    serde_json::to_value(v).map(Reply::value).map_err(|e| e.to_string())
}

impl Host {
    fn request(self: &Arc<Self>, link: &Arc<Link>, f: Frame) {
        if f.head["t"] != "req" {
            return;
        }
        let id = f.head["id"].as_u64().unwrap_or(0);
        let cmd = f.head["cmd"].as_str().unwrap_or("").to_string();
        let args = f.head["args"].clone();
        match cmd.as_str() {
            // Debug builds only: a stream that starts and then never says anything, as a stuck job would.
            #[cfg(debug_assertions)]
            "silent" => link.reply(id, Ok(Reply::value(Value::Null))),
            // Debug builds only: stops reading the app's messages at all, as a frozen engine would.
            #[cfg(debug_assertions)]
            "freeze" => loop {
                std::thread::sleep(Duration::from_secs(3600));
            },
            // These only queue work (answered as it's done): on this thread.
            "render" | "search" | "reflow" | "cancel" => {
                if let Err(e) = self.queue(link, id, &cmd, &args) {
                    link.reply(id, Err(e));
                }
            }
            // The rest may take a while (or ask the app for a file): a thread each.
            _ => {
                let (h, l) = (self.clone(), link.clone());
                let mut fds = f.fds;
                std::thread::spawn(move || {
                    let res = h.run(&cmd, &args, &mut fds);
                    l.reply(id, res);
                });
            }
        }
    }

    fn queue(&self, link: &Arc<Link>, id: u64, cmd: &str, args: &Value) -> Result<(), String> {
        let e = &self.engine;
        match cmd {
            "render" => {
                let req = RenderReq { page: arg(args, "page")?, scale: arg(args, "scale")?, rotate: arg(args, "rotate")?, clip: arg(args, "clip")?, extras: arg(args, "extras")? };
                let l = link.clone();
                e.render(arg(args, "doc")?, req, arg(args, "rid")?, arg(args, "prio")?, move |res| {
                    l.reply(
                        id,
                        res.map(|r| match r {
                            Some(r) => Reply { v: json!("pixels"), bin: r.bytes, fds: Vec::new() },
                            None => Reply::value(json!("cancelled")),
                        }),
                    )
                });
            }
            "search" => {
                let sid: u64 = arg(args, "sid")?;
                let opts: SearchOptions = arg::<Option<SearchOptions>>(args, "opts")?.unwrap_or_default();
                let (l1, l2, l3) = (link.clone(), link.clone(), link.clone());
                e.search(
                    arg(args, "doc")?,
                    arg(args, "needle")?,
                    opts,
                    sid,
                    move |page, hits| l1.event(sid, json!(SearchEvent::Hits { page, hits: hits.quads, snippets: hits.snippets })),
                    move |from, to| l2.event(sid, json!(SearchEvent::Scanned { from, to })),
                    move |end| l3.event(sid, json!(SearchEvent::Done { complete: end.complete, error: end.error, limited: end.limited })),
                )?;
                link.reply(id, Ok(Reply::value(Value::Null)));
            }
            "reflow" => {
                let sid: u64 = arg(args, "sid")?;
                let (l1, l2) = (link.clone(), link.clone());
                e.reflow(
                    arg(args, "doc")?,
                    arg(args, "start")?,
                    sid,
                    move |page, data| l1.event(sid, json!(ReflowEvent::Page { page, data })),
                    move |complete, error| l2.event(sid, json!(ReflowEvent::Done { complete, error })),
                )?;
                link.reply(id, Ok(Reply::value(Value::Null)));
            }
            _ => {
                for i in arg::<Vec<u64>>(args, "ids")? {
                    e.cancel(i);
                }
                link.reply(id, Ok(Reply::value(Value::Null)));
            }
        }
        Ok(())
    }

    fn run(&self, cmd: &str, args: &Value, fds: &mut Vec<OwnedFd>) -> Result<Reply, String> {
        let e = &self.engine;
        let doc = || arg::<DocId>(args, "doc");
        let page = || arg::<usize>(args, "page");
        // A file the app passed, named as the engine reads it ("fd:N/name").
        let mut given = || -> Result<(OwnedFd, String), String> {
            let fd = fds.pop().ok_or("No file was given.")?;
            let name: String = arg(args, "name")?;
            let path = sandbox::fd_path(fd.as_raw_fd(), &name.replace('/', "_"));
            Ok((fd, path))
        };
        match cmd {
            "open" => {
                let (fd, path) = given()?;
                let info = e.open(&path)?;
                self.files.lock().insert(info.doc_id, fd);
                to_value(info)
            }
            "unlock" => to_value(e.unlock(doc()?, &arg::<String>(args, "password")?)?),
            "close" => {
                let d = doc()?;
                e.close(d);
                self.files.lock().remove(&d);
                Ok(Reply::value(Value::Null))
            }
            "trim" => {
                e.trim(doc()?);
                Ok(Reply::value(Value::Null))
            }
            "properties" => to_value(e.doc(doc()?)?.properties()),
            "security" => to_value(e.security(doc()?)?),
            "page_sizes" => to_value(e.page_sizes(doc()?)?),
            "page_label" => to_value(e.doc(doc()?)?.label(page()?)?),
            "page_labels" => to_value(e.page_labels(doc()?)?),
            "outline" => to_value(app::outline_of(e, doc()?)?),
            "page_links" => to_value(app::page_links_of(e, doc()?, page()?)?),
            "page_images" => to_value(e.doc(doc()?)?.image_rects(page()?)?),
            "text_select" => to_value(app::text_select_of(e, doc()?, page()?, arg(args, "a")?, arg(args, "b")?, arg(args, "mode")?)?),
            "text_copy" => to_value(app::text_copy_of(e, doc()?, page()?, arg(args, "a")?, arg(args, "b")?)?),
            "text_range" => to_value(app::text_range_of(e, doc()?, page()?, arg(args, "p")?, arg(args, "mode")?)?),
            "text_range_copy" => to_value(app::text_range_copy_of(e, doc()?, page()?, arg(args, "from")?, arg(args, "to")?)?),
            "page_text" => to_value(e.doc(doc()?)?.text(page()?)?.lock().all()),
            "page_text_stats" => to_value(json!({ "chars": e.doc(doc()?)?.text(page()?)?.lock().char_count() })),
            "annots" => to_value(e.doc(doc()?)?.annots_json(page()?)?),
            "doc_annots" => to_value(e.doc(doc()?)?.doc_annots_json()?),
            "cover" => {
                let (_fd, path) = given()?;
                let (meta, jpg) = app::book_cover_of(e, &path)?;
                Ok(Reply { v: meta, bin: jpg.unwrap_or_default(), fds: Vec::new() })
            }
            "save_repaired" => {
                let (_fd, path) = given()?;
                e.save_repaired(doc()?, &path, arg(args, "pdf")?)?;
                Ok(Reply::value(Value::Null))
            }
            "export_pdf" => {
                let (_fd, path) = given()?;
                e.export_pdf(doc()?, &path)?;
                Ok(Reply::value(Value::Null))
            }
            // Debug builds only: stops the engine as a crash would (tools/android-engine-test.sh).
            #[cfg(debug_assertions)]
            "crash" => std::process::abort(),
            // Debug builds only: asks the app for `n` temporary files (how many it got, or its refusal).
            #[cfg(debug_assertions)]
            "flood" => {
                use sandbox::Scratch;
                let n: u64 = arg(args, "n")?;
                for i in 0..n {
                    if let Err(e) = AppFiles.anon() {
                        return Ok(Reply::value(json!({ "got": i, "error": e })));
                    }
                }
                Ok(Reply::value(json!({ "got": n })))
            }
            // Debug builds only: stops answering, as a file that makes the engine loop would.
            #[cfg(debug_assertions)]
            "hang" => loop {
                std::thread::sleep(Duration::from_secs(3600));
            },
            _ => Err(format!("The engine doesn't know \"{cmd}\".")),
        }
    }

    /// The app has gone (or started another engine): this connection's documents close.
    fn end(&self) {
        let ids: Vec<DocId> = self.files.lock().keys().copied().collect();
        for id in ids {
            self.engine.close(id);
        }
        self.files.lock().clear();
    }
}

// ---------------------------------------------------------------- the app's side

/// A document as the page knows it: the file to open again, and what it is in the current engine.
struct Opened {
    path: String,
    password: Option<String>,
    /// Which engine (`Remote::gen`) has it open, as `server`.
    gen: u64,
    server: DocId,
    refused: bool,
}

/// A search or Read mode's pages, streaming to the page.
struct Stream {
    doc: DocId,
    cmd: &'static str,
    args: Value,
    ch: Channel<Value>,
    /// Pages already sent: a stream started again on a new engine doesn't send them twice.
    seen: HashSet<usize>,
    gen: u64,
    /// When the engine last said something about it (the watchdog's measure for streams).
    last: Instant,
    /// Being started (start_stream tries again itself if the engine stops meanwhile).
    starting: bool,
}

/// How long a stream may go without an event before its engine counts as stuck: searches and Read mode
/// report every few pages, however long the whole job.
const STREAM_SILENCE: Duration = Duration::from_secs(90);

pub struct Remote {
    /// Starts an engine (EngineService) and gives the app's end of its socket.
    connect: Box<dyn Fn() -> Result<RawFd, String> + Send + Sync>,
    /// The engine's kept files (books drawn again), in the app's cache.
    files_dir: PathBuf,
    link: Mutex<Option<(Arc<Link>, u64)>>,
    gen: AtomicU64,
    docs: Mutex<HashMap<DocId, Opened>>,
    next_doc: AtomicU64,
    /// Documents being opened again on a new engine, one at a time.
    reopening: Mutex<()>,
    streams: Mutex<HashMap<u64, Stream>>,
    /// Told when a document is refused (the page leaves it and says why).
    on_refused: Box<dyn Fn(DocId) + Send + Sync>,
    /// When each document was being worked on as an engine stopped, and which engine.
    strikes: Mutex<HashMap<DocId, Vec<(Instant, u64)>>>,
    /// Covers being drawn for the shelf (the book's path, and on which engine), the engines that stopped with one
    /// under way, and how often each book's cover has stopped an engine. A cover has no document to count a
    /// strike against, so an engine that stops while one is drawn is put down to that book, not to the document
    /// being read: twice, and the book's cover isn't tried again until Paginaut starts anew.
    covering: Mutex<HashMap<String, u64>>,
    cover_stopped: Mutex<std::collections::HashSet<u64>>,
    bad_covers: Mutex<HashMap<String, u32>>,
    /// Calls waiting for the engine: when they started, what they are, and on which engine (`watch`).
    waiting: Mutex<HashMap<u64, (Instant, &'static str, u64)>>,
    next_wait: AtomicU64,
    /// Temporary files the current engine has asked for, and the free space when it started.
    temp_made: AtomicU64,
    free_at_start: AtomicU64,
    me: Weak<Remote>,
}

/// A file for the engine: a handle of the app's own reading it (big files read in place are links to a handle
/// Android lent: that handle is shared, as opening the link again is refused), and its name.
fn file_for_engine(path: &str) -> Result<(OwnedFd, String), String> {
    let name = Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "document".into());
    if let Ok(target) = std::fs::read_link(path) {
        if let Some(n) = target.to_str().and_then(|t| t.strip_prefix("/proc/self/fd/")).and_then(|n| n.parse::<RawFd>().ok()) {
            // SAFETY: the plugin keeps that handle open while the document is open (PaginautPlugin.release).
            let fd = unsafe { BorrowedFd::borrow_raw(n) }.try_clone_to_owned().map_err(|e| e.to_string())?;
            return Ok((fd, name));
        }
    }
    let f = std::fs::File::open(path).map_err(|e| format!("Couldn't read the file: {e}"))?;
    Ok((OwnedFd::from(f), name))
}

/// A name for one of the engine's files: plain characters only (it becomes a path in the app's cache).
fn safe_name(name: &str) -> Result<&str, String> {
    let ok = !name.is_empty() && !name.starts_with('.') && name.len() < 128 && name.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
    ok.then_some(name).ok_or_else(|| "bad file name".into())
}

impl Remote {
    pub fn new(connect: impl Fn() -> Result<RawFd, String> + Send + Sync + 'static, cache: &Path, on_refused: impl Fn(DocId) + Send + Sync + 'static) -> Arc<Remote> {
        let files_dir = cache.join("engine");
        // Books drawn again by an earlier run go, as on the desktop (pdfhtml::clear).
        let _ = std::fs::remove_dir_all(&files_dir);
        let _ = std::fs::create_dir_all(&files_dir);
        Arc::new_cyclic(|me| Remote {
            connect: Box::new(connect),
            files_dir,
            link: Mutex::new(None),
            gen: AtomicU64::new(0),
            docs: Mutex::new(HashMap::new()),
            next_doc: AtomicU64::new(1),
            reopening: Mutex::new(()),
            streams: Mutex::new(HashMap::new()),
            on_refused: Box::new(on_refused),
            strikes: Mutex::new(HashMap::new()),
            covering: Mutex::new(HashMap::new()),
            cover_stopped: Mutex::new(std::collections::HashSet::new()),
            bad_covers: Mutex::new(HashMap::new()),
            waiting: Mutex::new(HashMap::new()),
            next_wait: AtomicU64::new(1),
            temp_made: AtomicU64::new(0),
            free_at_start: AtomicU64::new(0),
            me: me.clone(),
        })
    }

    /// How long a call may wait before the engine counts as stuck (a hostile file can make it loop
    /// rather than stop): opening and writing a whole book take longer than a page.
    fn patience(cmd: &str) -> Duration {
        match cmd {
            "open" | "unlock" | "export_pdf" | "save_repaired" => Duration::from_secs(180),
            "cover" => Duration::from_secs(60),
            _ => Duration::from_secs(45),
        }
    }

    /// Runs `f` (a call to engine `gen`) counted as waiting, for the watchdog.
    fn waiting<T>(&self, cmd: &'static str, gen: u64, f: impl FnOnce() -> T) -> T {
        let token = self.next_wait.fetch_add(1, Ordering::Relaxed);
        self.waiting.lock().insert(token, (Instant::now(), cmd, gen));
        let res = f();
        self.waiting.lock().remove(&token);
        res
    }

    /// Every few seconds: an engine that has kept a call waiting too long is let go, as if it had stopped
    /// (its calls get STOPPED, are tried again on a new engine, and count against their documents). So is one
    /// filling the phone through its temporary files (the app can't limit what is written into a file once
    /// handed over): when it ends, Android frees its nameless files at once, and its kept ones go here.
    fn watch(self: &Arc<Self>) {
        let me = Arc::downgrade(self);
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(5));
            let Some(r) = me.upgrade() else { return };
            let Some((link, gen)) = r.link.lock().clone() else { continue };
            let stuck = r.waiting.lock().values().any(|(at, cmd, g)| *g == gen && at.elapsed() > Remote::patience(cmd))
                || r.streams.lock().values().any(|s| s.gen == gen && !s.starting && s.last.elapsed() > STREAM_SILENCE);
            if stuck || r.filling() {
                let what = r.waiting.lock().values().filter(|(_, _, g)| *g == gen).map(|(_, cmd, _)| *cmd).next().unwrap_or("reading");
                crate::errlog::write("engine", &if stuck { format!("Engine {gen} stopped answering ({what}): stopped and started again.") } else { format!("Engine {gen} was filling the phone's storage: stopped.") });
                link.close();
                if !stuck {
                    let _ = std::fs::remove_dir_all(&r.files_dir);
                    let _ = std::fs::create_dir_all(&r.files_dir);
                }
            }
        });
    }

    /// Free and total bytes where the engine's files are.
    fn space(&self) -> Option<(u64, u64)> {
        let path = std::ffi::CString::new(self.files_dir.to_string_lossy().as_bytes()).ok()?;
        // SAFETY: statvfs fills the struct it is given.
        let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statvfs(path.as_ptr(), &mut st) } != 0 {
            return None;
        }
        let unit = st.f_frsize as u64;
        Some((st.f_bavail as u64 * unit, st.f_blocks as u64 * unit))
    }

    /// The phone is nearly full: under 1 GB free, or 5% of its storage on a small one.
    fn low_on_space(&self) -> bool {
        self.space().is_some_and(|(free, total)| free < (1u64 << 30).min(total / 20))
    }

    /// The engine has made temporary files, the phone is nearly full, and it has filled up since the engine
    /// started: that engine's doing.
    fn filling(&self) -> bool {
        if self.temp_made.load(Ordering::Relaxed) == 0 || !self.low_on_space() {
            return false;
        }
        let (start, now) = (self.free_at_start.load(Ordering::Relaxed), self.space().map_or(u64::MAX, |s| s.0));
        start.saturating_sub(now) > MAX_ENGINE_GROWTH
    }

    /// The engine, started (again) if it isn't running.
    fn link(&self) -> Result<(Arc<Link>, u64), String> {
        let mut cur = self.link.lock();
        if let Some((l, g)) = cur.as_ref() {
            if l.alive() {
                return Ok((l.clone(), *g));
            }
        }
        if let Some((old, g)) = cur.take() {
            old.close();
            crate::errlog::write("engine", &format!("Engine {g} had stopped (a damaged file, or Android reclaiming memory): starting another."));
        }
        let fd = (self.connect)().inspect_err(|e| crate::errlog::write("engine", &format!("The engine didn't start: {e}")))?;
        // SAFETY: the plugin detached this handle for us.
        let sock = unsafe { UnixStream::from_raw_fd(fd) };
        let gen = self.gen.fetch_add(1, Ordering::SeqCst) + 1;
        self.temp_made.store(0, Ordering::Relaxed);
        self.free_at_start.store(self.space().map_or(0, |s| s.0), Ordering::Relaxed);
        let (me1, me2) = (self.me.clone(), self.me.clone());
        let link = Link::start(
            sock,
            move |l, f| {
                if let Some(r) = me1.upgrade() {
                    r.from_engine(l, f)
                }
            },
            move || {
                if let Some(r) = me2.upgrade() {
                    r.engine_stopped(gen)
                }
            },
        )
        .map_err(|e| e.to_string())?;
        *cur = Some((link.clone(), gen));
        Ok((link, gen))
    }

    /// Starts the engine ahead of the first document (it takes a moment), and the watchdog.
    pub fn warm_up(self: &Arc<Self>) {
        self.watch();
        let r = self.clone();
        std::thread::spawn(move || {
            let _ = r.link();
        });
    }

    /// What the engine asks of the app: files (sandbox::Scratch), and events of streams.
    fn from_engine(&self, link: &Arc<Link>, f: Frame) {
        match f.head["t"].as_str() {
            Some("ev") => self.stream_event(f.head["sid"].as_u64().unwrap_or(0), f.head["v"].clone()),
            Some("req") => {
                let id = f.head["id"].as_u64().unwrap_or(0);
                let res = self.scratch(&f.head["args"]);
                link.reply(id, res);
            }
            _ => {}
        }
    }

    fn scratch(&self, args: &Value) -> Result<Reply, String> {
        let op = args["op"].as_str().unwrap_or("");
        let named = || -> Result<PathBuf, String> { Ok(self.files_dir.join(safe_name(args["name"].as_str().unwrap_or(""))?)) };
        let part = |p: &Path| PathBuf::from(format!("{}.part", p.display()));
        let rw = |p: &Path| std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(true).open(p);
        let with = |fd: OwnedFd| Reply { v: Value::Null, bin: Vec::new(), fds: vec![fd] };
        // New files: so many per engine, and none on a nearly full phone.
        if op == "anon" || op == "create" {
            if self.temp_made.fetch_add(1, Ordering::Relaxed) >= MAX_TEMP_FILES {
                return Err("The reading engine asked for too many temporary files.".into());
            }
            if self.low_on_space() {
                return Err("The phone is nearly out of space.".into());
            }
        }
        match op {
            "anon" => {
                static N: AtomicU64 = AtomicU64::new(0);
                let p = self.files_dir.join(format!("anon-{}", N.fetch_add(1, Ordering::Relaxed)));
                let f = rw(&p).map_err(|e| e.to_string())?;
                // Nameless from now on: it goes when the engine lets go of it.
                let _ = std::fs::remove_file(&p);
                Ok(with(f.into()))
            }
            "open" => match std::fs::File::open(named()?) {
                Ok(f) => Ok(with(f.into())),
                Err(_) => Ok(Reply::value(Value::Null)),
            },
            "create" => Ok(with(rw(&part(&named()?)).map_err(|e| e.to_string())?.into())),
            "commit" => {
                let p = named()?;
                std::fs::rename(part(&p), &p).map(|_| Reply::value(Value::Null)).map_err(|e| e.to_string())
            }
            _ => Err("unknown file request".into()),
        }
    }

    fn stream_event(&self, sid: u64, mut v: Value) {
        let mut streams = self.streams.lock();
        let Some(s) = streams.get_mut(&sid) else { return };
        s.last = Instant::now();
        let page = v["page"].as_u64().map(|p| p as usize);
        match v["kind"].as_str() {
            Some("hits") | Some("page") => {
                if let Some(p) = page {
                    if !s.seen.insert(p) {
                        return;
                    }
                }
            }
            Some("done") => {
                let s = streams.remove(&sid).unwrap();
                drop(streams);
                let _ = s.ch.send(v);
                return;
            }
            _ => {}
        }
        let _ = s.ch.send(std::mem::take(&mut v));
    }

    /// The engine `gen` stopped: what it was streaming starts again on a new one (not streams being started:
    /// start_stream tries those again itself).
    fn engine_stopped(&self, gen: u64) {
        let sids: Vec<u64> = self.streams.lock().iter().filter(|(_, s)| s.gen == gen && !s.starting).map(|(id, _)| *id).collect();
        if sids.is_empty() {
            return;
        }
        let Some(me) = self.me.upgrade() else { return };
        std::thread::spawn(move || {
            for sid in sids {
                let doc = match me.streams.lock().get(&sid) {
                    Some(s) => s.doc,
                    None => continue,
                };
                me.strike(doc, gen);
                if let Err(e) = me.start_stream(sid) {
                    if let Some(s) = me.streams.lock().remove(&sid) {
                        let _ = s.ch.send(json!({ "kind": "done", "complete": false, "error": e, "limited": false }));
                    }
                }
            }
        });
    }

    /// The document was being worked on as engine `gen` stopped (counted once per engine, however many
    /// pages were being drawn). Twice within two minutes: it's refused.
    fn strike(&self, doc: DocId, gen: u64) {
        // A cover was being drawn on that engine: the book on the shelf is the suspect (see `covering`), not this
        // document, which would otherwise be closed for a damaged book it has nothing to do with.
        if self.covering.lock().values().any(|g| *g == gen) || self.cover_stopped.lock().contains(&gen) {
            return;
        }
        let now = Instant::now();
        let mut strikes = self.strikes.lock();
        let list = strikes.entry(doc).or_default();
        list.retain(|(t, _)| now.duration_since(*t) < Duration::from_secs(120));
        if list.iter().any(|(_, g)| *g == gen) {
            return;
        }
        list.push((now, gen));
        let twice = list.len() >= 2;
        drop(strikes);
        let newly = twice && self.docs.lock().get_mut(&doc).is_some_and(|o| !std::mem::replace(&mut o.refused, true));
        if newly {
            crate::errlog::write("engine", "A document stopped the engine twice within two minutes: refused.");
            (self.on_refused)(doc);
        }
    }

    /// Opens `path` in the engine `link`: its DocInfo.
    fn open_on(&self, link: &Link, gen: u64, path: &str) -> Result<Value, String> {
        let (fd, name) = file_for_engine(path)?;
        let f = self.waiting("open", gen, || link.call("open", json!({ "name": name }), &[], &[fd.as_raw_fd()]))?;
        Ok(f.head["v"].clone())
    }

    /// The document `doc` in the current engine: opened again there if the engine is new.
    fn server_doc(&self, link: &Link, gen: u64, doc: DocId) -> Result<DocId, String> {
        {
            let docs = self.docs.lock();
            let o = docs.get(&doc).ok_or_else(|| format!("document {doc} is not open"))?;
            if o.refused {
                return Err(REFUSED.into());
            }
            if o.gen == gen {
                return Ok(o.server);
            }
        }
        let _one = self.reopening.lock();
        let (path, password) = {
            let docs = self.docs.lock();
            let o = docs.get(&doc).ok_or_else(|| format!("document {doc} is not open"))?;
            if o.gen == gen {
                return Ok(o.server);
            }
            (o.path.clone(), o.password.clone())
        };
        let info = self.open_on(link, gen, &path)?;
        let server = info["docId"].as_u64().ok_or("The engine didn't open it.")?;
        if let Some(pw) = password {
            let unlocked = self.waiting("unlock", gen, || link.call("unlock", json!({ "doc": server, "password": pw }), &[], &[]))?;
            // The password that opened it no longer does (the file changed meanwhile): its pages can't be read.
            if unlocked.head["v"].is_null() {
                let _ = self.waiting("close", gen, || link.call("close", json!({ "doc": server }), &[], &[]));
                return Err("This file has changed: its password no longer opens it. Open it again.".into());
            }
        }
        if let Some(o) = self.docs.lock().get_mut(&doc) {
            o.gen = gen;
            o.server = server;
        }
        Ok(server)
    }

    /// Calls the engine about `doc` (`args.doc` set to the engine's own number for it); if the engine stops
    /// meanwhile, once more on a new one.
    fn call_doc(&self, cmd: &'static str, doc: DocId, mut args: Value, fds: &[RawFd]) -> Result<Frame, String> {
        let mut tries = 0;
        loop {
            let (link, gen) = self.link()?;
            let res = self.server_doc(&link, gen, doc).and_then(|server| {
                args["doc"] = json!(server);
                self.waiting(cmd, gen, || link.call(cmd, args.clone(), &[], fds))
            });
            match res {
                Err(e) if e == STOPPED => {
                    self.strike(doc, gen);
                    if self.docs.lock().get(&doc).is_some_and(|o| o.refused) {
                        return Err(REFUSED.into());
                    }
                    if tries > 0 {
                        return Err(e);
                    }
                    tries += 1;
                }
                res => return res,
            }
        }
    }

    // ------------------------------------------------ what the page calls (mobile.rs)

    pub fn open(&self, path: &str) -> Result<Value, String> {
        let mut tries = 0;
        loop {
            let (link, gen) = self.link()?;
            match self.open_on(&link, gen, path) {
                Ok(mut info) => {
                    let server = info["docId"].as_u64().ok_or("The engine didn't open it.")?;
                    let id = self.next_doc.fetch_add(1, Ordering::Relaxed);
                    self.docs.lock().insert(id, Opened { path: path.to_string(), password: None, gen, server, refused: false });
                    info["docId"] = json!(id);
                    return Ok(info);
                }
                // Stopped while opening: once more; a file that stops it again isn't tried a third time.
                Err(e) if e == STOPPED && tries == 0 => tries += 1,
                Err(e) if e == STOPPED => return Err(REFUSED.into()),
                Err(e) => return Err(e),
            }
        }
    }

    pub fn unlock(&self, doc: DocId, password: &str) -> Result<Value, String> {
        let mut info = self.call_doc("unlock", doc, json!({ "password": password }), &[])?.head["v"].clone();
        if !info.is_null() {
            info["docId"] = json!(doc);
            if let Some(o) = self.docs.lock().get_mut(&doc) {
                o.password = Some(password.to_string());
            }
        }
        Ok(info)
    }

    pub fn close(&self, doc: DocId) {
        let Some(o) = self.docs.lock().remove(&doc) else { return };
        self.strikes.lock().remove(&doc);
        // Taken out of the lock first: an `if let` would hold it through the call, and the watchdog needs it to
        // let a frozen engine go.
        let current = self.link.lock().clone();
        if let Some((link, gen)) = current {
            if gen == o.gen && link.alive() {
                let _ = self.waiting("close", gen, || link.call("close", json!({ "doc": o.server }), &[], &[]));
            }
        }
    }

    /// A command about one document that answers with a value.
    pub fn doc_value(&self, cmd: &'static str, doc: DocId, args: Value) -> Result<Value, String> {
        Ok(self.call_doc(cmd, doc, args, &[])?.head["v"].clone())
    }

    /// Page pixels (8-byte size header + RGBA), or None if the drawing was cancelled.
    pub fn render(&self, doc: DocId, args: Value) -> Result<Option<Vec<u8>>, String> {
        let f = self.call_doc("render", doc, args, &[])?;
        Ok((f.head["v"] == "pixels").then_some(f.bin))
    }

    /// A search or Read mode's pages: streamed to `ch` (started again on a new engine if this one stops).
    pub fn stream(&self, cmd: &'static str, doc: DocId, sid: u64, args: Value, ch: Channel<Value>) -> Result<(), String> {
        let s = Stream { doc, cmd, args, ch, seen: HashSet::new(), gen: 0, last: Instant::now(), starting: true };
        self.streams.lock().insert(sid, s);
        self.start_stream(sid).inspect_err(|_| {
            self.streams.lock().remove(&sid);
        })
    }

    /// Starts (or starts again) stream `sid` on the current engine. An engine that stops meanwhile is
    /// this function's to deal with (engine_stopped leaves streams being started alone): once more on a
    /// new one, then an error.
    fn start_stream(&self, sid: u64) -> Result<(), String> {
        let (doc, cmd, base) = match self.streams.lock().get_mut(&sid) {
            Some(s) => {
                s.starting = true;
                (s.doc, s.cmd, s.args.clone())
            }
            None => return Ok(()),
        };
        let mut tries = 0;
        let res = loop {
            let (link, gen) = match self.link() {
                Ok(l) => l,
                Err(e) => break Err(e),
            };
            let res = self.server_doc(&link, gen, doc).and_then(|server| {
                let mut args = base.clone();
                args["doc"] = json!(server);
                args["sid"] = json!(sid);
                if let Some(s) = self.streams.lock().get_mut(&sid) {
                    s.gen = gen;
                    s.last = Instant::now();
                }
                self.waiting(cmd, gen, || link.call(cmd, args, &[], &[]))
            });
            // Stopped while starting, or just after (before engine_stopped could see it as started).
            let stopped = matches!(&res, Err(e) if e == STOPPED) || (res.is_ok() && !link.alive());
            if !stopped {
                break res.map(|_| ());
            }
            self.strike(doc, gen);
            if tries > 0 || self.docs.lock().get(&doc).is_some_and(|o| o.refused) {
                break Err(if self.docs.lock().get(&doc).is_some_and(|o| o.refused) { REFUSED.into() } else { STOPPED.into() });
            }
            tries += 1;
        };
        if let Some(s) = self.streams.lock().get_mut(&sid) {
            s.starting = false;
        }
        res
    }

    pub fn cancel(&self, ids: Vec<u64>) {
        {
            let mut streams = self.streams.lock();
            for id in &ids {
                streams.remove(id);
            }
        }
        let current = self.link.lock().clone();
        if let Some((link, gen)) = current {
            if link.alive() {
                let _ = self.waiting("cancel", gen, || link.call("cancel", json!({ "ids": ids }), &[], &[]));
            }
        }
    }

    /// Stops the engine as a crash would ("crash"), or makes it stop answering ("hang"): debug builds,
    /// tools/android-engine-test.sh. A hang returns once the watchdog has let that engine go.
    pub fn trouble_for_test(&self, how: &str, doc: Option<DocId>) -> Result<Value, String> {
        let (link, gen) = self.link()?;
        match how {
            // A stream on `doc` that goes quiet: answers once the watchdog has started a new engine for it.
            "silent" => {
                let doc = doc.ok_or("silent needs a document")?;
                let sid = u64::MAX - gen;
                self.stream("silent", doc, sid, Value::Null, Channel::new(|_| Ok(())))?;
                let from = Instant::now();
                while self.gen.load(Ordering::SeqCst) == gen && from.elapsed() < Duration::from_secs(150) {
                    std::thread::sleep(Duration::from_millis(250));
                }
                self.cancel(vec![sid]);
                Ok(json!({ "seconds": from.elapsed().as_secs(), "newEngine": self.gen.load(Ordering::SeqCst) != gen }))
            }
            "hang" => self.waiting("hang", gen, || link.call("hang", Value::Null, &[], &[])).map(|_| Value::Null),
            // Not counted as waiting: whatever the app asks next is what the watchdog has to notice.
            "freeze" => link.call("freeze", Value::Null, &[], &[]).map(|_| Value::Null),
            "flood" => link.call("flood", json!({ "n": MAX_TEMP_FILES + 20 }), &[], &[]).map(|f| f.head["v"].clone()),
            _ => link.call("crash", Value::Null, &[], &[]).map(|_| Value::Null),
        }
    }

    /// A book's title, author and pages, and its cover saved as covers/<fp>.jpg.
    pub fn cover(&self, path: &str, covers: &Path, fp: &str) -> Result<Value, String> {
        if self.bad_covers.lock().get(path).is_some_and(|n| *n >= 2) {
            return Err("This book's cover stopped Paginaut's reading engine twice, so it isn't drawn. The file may be damaged.".into());
        }
        let (link, gen) = self.link()?;
        let (fd, name) = file_for_engine(path)?;
        self.covering.lock().insert(path.to_string(), gen);
        let res = self.waiting("cover", gen, || link.call("cover", json!({ "name": name }), &[], &[fd.as_raw_fd()]));
        if res.as_ref().is_err_and(|e| e == STOPPED) {
            // Noted before the cover stops counting as under way, so `strike` sees one or the other.
            self.cover_stopped.lock().insert(gen);
            *self.bad_covers.lock().entry(path.to_string()).or_default() += 1;
        }
        self.covering.lock().remove(path);
        let f = res?;
        let jpg = (!f.bin.is_empty()).then_some(f.bin);
        Ok(app::save_cover(f.head["v"].clone(), jpg, covers, fp))
    }

    /// Writes the document (`cmd`: "export_pdf", "save_repaired") into `target`, a file the app makes.
    pub fn write_to(&self, cmd: &'static str, doc: DocId, target: &str, mut args: Value) -> Result<(), String> {
        if let Some(dir) = Path::new(target).parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let file = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(true).open(target).map_err(|e| e.to_string())?;
        args["name"] = json!(Path::new(target).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        let res = self.call_doc(cmd, doc, args, &[file.as_raw_fd()]).map(|_| ());
        if res.is_err() {
            let _ = std::fs::remove_file(target);
        }
        res
    }
}
