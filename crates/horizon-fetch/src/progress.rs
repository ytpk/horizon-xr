use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::sync::Mutex;
use std::time::Instant;

/// A download's counters, shared with the screen that draws them. `const`-constructible, so a
/// program can keep one in a `static`.
pub struct Progress {
    files_done: AtomicUsize,
    files_total: AtomicUsize,
    bytes_done: AtomicU64,
    bytes_total: AtomicU64,
    /// Bytes that came over the network this run (not those already on disk): the speed's.
    bytes_fetched: AtomicU64,
    started: Mutex<Option<Instant>>,
}

/// What a screen shows: files and bytes done of how many, the speed and the time left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snapshot {
    pub files_done: usize,
    pub files_total: usize,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub bytes_per_sec: f64,
    /// None until the speed is known or the total size is.
    pub eta_secs: Option<u64>,
}

impl Progress {
    pub const fn new() -> Self {
        Self {
            files_done: AtomicUsize::new(0),
            files_total: AtomicUsize::new(0),
            bytes_done: AtomicU64::new(0),
            bytes_total: AtomicU64::new(0),
            bytes_fetched: AtomicU64::new(0),
            started: Mutex::new(None),
        }
    }

    /// A download starts: `files` and `bytes` to get through.
    pub fn begin(&self, files: usize, bytes: u64) {
        self.files_done.store(0, Relaxed);
        self.files_total.store(files, Relaxed);
        self.bytes_done.store(0, Relaxed);
        self.bytes_total.store(bytes, Relaxed);
        self.bytes_fetched.store(0, Relaxed);
        *self.started.lock().unwrap() = Some(Instant::now());
    }

    /// The download is over (or none runs): `snapshot` says `None` again.
    pub fn end(&self) {
        self.files_total.store(0, Relaxed);
        *self.started.lock().unwrap() = None;
    }

    pub(crate) fn file_done(&self) {
        self.files_done.fetch_add(1, Relaxed);
    }

    /// Bytes already on disk that count as done.
    pub(crate) fn skipped(&self, bytes: u64) {
        self.bytes_done.fetch_add(bytes, Relaxed);
    }

    pub(crate) fn fetched(&self, bytes: u64) {
        self.bytes_done.fetch_add(bytes, Relaxed);
        self.bytes_fetched.fetch_add(bytes, Relaxed);
    }

    /// The counters now; `None` while no download runs.
    pub fn snapshot(&self) -> Option<Snapshot> {
        let files_total = self.files_total.load(Relaxed);
        let started = (*self.started.lock().unwrap())?;
        if files_total == 0 {
            return None;
        }
        let bytes_total = self.bytes_total.load(Relaxed);
        let bytes_done = self.bytes_done.load(Relaxed).min(bytes_total.max(1));
        let secs = started.elapsed().as_secs_f64().max(0.001);
        let bytes_per_sec = self.bytes_fetched.load(Relaxed) as f64 / secs;
        let eta_secs = (bytes_per_sec >= 1.0 && bytes_total > bytes_done)
            .then(|| ((bytes_total - bytes_done) as f64 / bytes_per_sec).ceil() as u64);
        Some(Snapshot {
            files_done: self.files_done.load(Relaxed).min(files_total),
            files_total,
            bytes_done,
            bytes_total,
            bytes_per_sec,
            eta_secs,
        })
    }
}

impl Default for Progress {
    fn default() -> Self {
        Self::new()
    }
}
