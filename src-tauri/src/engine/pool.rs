//! Priority thread pool. Visible pages outrank prefetch, which outranks
//! thumbnails and background jobs. Every job carries a control block that
//! can cancel it before it starts or abort MuPDF while it is rendering.

use super::mupdf::Cookie;
use parking_lot::{Condvar, Mutex};
use std::collections::{BinaryHeap, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

pub struct Ctl {
    cancelled: AtomicBool,
    pub cookie: Cookie,
}

impl Ctl {
    pub fn new() -> Arc<Self> {
        Arc::new(Ctl { cancelled: AtomicBool::new(false), cookie: Cookie::new() })
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.cookie.abort();
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

type Work = Box<dyn FnOnce(&Ctl) + Send>;

struct Job {
    prio: i32,
    seq: u64,
    id: u64,
    ctl: Arc<Ctl>,
    work: Work,
}

impl PartialEq for Job {
    fn eq(&self, o: &Self) -> bool {
        self.prio == o.prio && self.seq == o.seq
    }
}
impl Eq for Job {}
impl PartialOrd for Job {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Job {
    // Highest priority first; FIFO within a priority.
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        self.prio.cmp(&o.prio).then_with(|| o.seq.cmp(&self.seq))
    }
}

struct Shared {
    /// Bumped to make every worker run `maintenance` (e.g. drop private copies of closed documents).
    epoch: AtomicU64,
    maintenance: Arc<dyn Fn() + Send + Sync>,
    queue: Mutex<BinaryHeap<Job>>,
    ready: Condvar,
    shutdown: AtomicBool,
    live: Mutex<HashMap<u64, Arc<Ctl>>>,
    seq: AtomicU64,
    busy: AtomicUsize,
}

pub struct Pool {
    shared: Arc<Shared>,
    threads: Vec<JoinHandle<()>>,
}

impl Pool {
    pub fn new(workers: usize, maintenance: Arc<dyn Fn() + Send + Sync>) -> Self {
        let shared = Arc::new(Shared {
            epoch: AtomicU64::new(0),
            maintenance,
            queue: Mutex::new(BinaryHeap::new()),
            ready: Condvar::new(),
            shutdown: AtomicBool::new(false),
            live: Mutex::new(HashMap::new()),
            seq: AtomicU64::new(0),
            busy: AtomicUsize::new(0),
        });
        let threads = (0..workers)
            .map(|i| {
                let s = shared.clone();
                std::thread::Builder::new()
                    .name(format!("folio-worker-{i}"))
                    .stack_size(8 << 20)
                    .spawn(move || worker(s))
                    .expect("spawn worker")
            })
            .collect();
        Pool { shared, threads }
    }

    pub fn workers(&self) -> usize {
        self.threads.len()
    }

    pub fn busy(&self) -> usize {
        self.shared.busy.load(Ordering::Relaxed)
    }

    pub fn queued(&self) -> usize {
        self.shared.queue.lock().len()
    }

    /// Queues work under `id` (0 = anonymous). Jobs sharing an id share a
    /// control block, so one cancel stops all of them.
    pub fn submit(&self, id: u64, prio: i32, work: impl FnOnce(&Ctl) + Send + 'static) -> Arc<Ctl> {
        let ctl = if id == 0 {
            Ctl::new()
        } else {
            self.shared.live.lock().entry(id).or_insert_with(Ctl::new).clone()
        };
        let job = Job {
            prio,
            seq: self.shared.seq.fetch_add(1, Ordering::Relaxed),
            id,
            ctl: ctl.clone(),
            work: Box::new(work),
        };
        self.shared.queue.lock().push(job);
        self.shared.ready.notify_one();
        ctl
    }

    pub fn cancel(&self, id: u64) {
        if let Some(ctl) = self.shared.live.lock().remove(&id) {
            ctl.cancel();
        }
    }

    /// Asks every worker to run the maintenance hook as soon as it is idle.
    pub fn maintain(&self) {
        self.shared.epoch.fetch_add(1, Ordering::AcqRel);
        self.shared.ready.notify_all();
    }

    /// Forgets the control block for `id` once all its jobs are done.
    pub fn finish(&self, id: u64) {
        self.shared.live.lock().remove(&id);
    }
}

fn worker(s: Arc<Shared>) {
    let mut epoch = 0;
    loop {
        let job = {
            let mut q = s.queue.lock();
            loop {
                let e = s.epoch.load(Ordering::Acquire);
                if e != epoch {
                    epoch = e;
                    drop(q);
                    (s.maintenance)();
                    q = s.queue.lock();
                    continue;
                }
                if s.shutdown.load(Ordering::Acquire) {
                    return;
                }
                if let Some(j) = q.pop() {
                    break j;
                }
                s.ready.wait(&mut q);
            }
        };
        s.busy.fetch_add(1, Ordering::Relaxed);
        let Job { ctl, work, id, .. } = job;
        // A panic inside one job must not take the worker down with it.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&ctl)));
        s.busy.fetch_sub(1, Ordering::Relaxed);
        if id != 0 && Arc::strong_count(&ctl) <= 2 {
            // Only the live map and our local copy remain: nothing else queued under this id.
            let mut live = s.live.lock();
            if live.get(&id).is_some_and(|c| Arc::ptr_eq(c, &ctl)) && Arc::strong_count(&ctl) <= 2 {
                live.remove(&id);
            }
        }
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.shared.shutdown.store(true, Ordering::Release);
        self.shared.ready.notify_all();
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}
