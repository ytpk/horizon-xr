use crate::{sha1_file, Error, Progress, Transport};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};
use std::sync::Mutex;
use std::time::Duration;

/// One file to download, with the size Mojang lists for it when it lists one.
#[derive(Clone, Debug)]
pub struct Item {
    pub url: String,
    pub sha1: String,
    pub size: Option<u64>,
    pub path: PathBuf,
}

/// How a batch is fetched.
pub struct Options {
    /// Files at a time (a few: each is a connection).
    pub jobs: usize,
    /// How many times a file whose connection fails is tried again, continuing where it stopped.
    pub retries: usize,
    /// Free bytes that must remain after the download (the Quest keeps room for preparing).
    pub reserve: u64,
    /// Free bytes on the disk holding a path; `None` skips the check. A test injects a full disk.
    pub free_space: fn(&Path) -> Option<u64>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            jobs: 4,
            retries: 2,
            reserve: 0,
            free_space,
        }
    }
}

/// Free bytes for unprivileged writes on the filesystem holding `path` (its nearest existing
/// parent); `None` where the system cannot say.
pub fn free_space(path: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let mut at = path;
        while !at.exists() {
            at = at.parent()?;
        }
        let c = std::ffi::CString::new(at.as_os_str().as_bytes()).ok()?;
        let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statvfs(c.as_ptr(), &mut stat) } != 0 {
            return None;
        }
        Some((stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

fn partial_path(path: &Path) -> PathBuf {
    path.with_file_name(format!(
        "{}.partial",
        path.file_name().unwrap().to_string_lossy()
    ))
}

fn partial_len(path: &Path) -> u64 {
    fs::metadata(partial_path(path)).map_or(0, |m| m.len())
}

/// Downloads the files not already there (whole and with the listed sha1), stopping at the first
/// error, or between chunks once `cancel` is set.
pub fn fetch_all(
    items: &[Item],
    options: &Options,
    transport: &dyn Transport,
    progress: &Progress,
    cancel: &AtomicBool,
    log: &dyn Fn(String),
) -> Result<(), Error> {
    fetch_each(items, options, transport, progress, cancel, log, true)
}

/// `fetch_all`, which with `stop_on_error` false goes on past a file that fails and reports how
/// many did, with the first one's error, once the rest are done.
pub fn fetch_each(
    items: &[Item],
    options: &Options,
    transport: &dyn Transport,
    progress: &Progress,
    cancel: &AtomicBool,
    log: &dyn Fn(String),
    stop_on_error: bool,
) -> Result<(), Error> {
    let listed: u64 = items.iter().filter_map(|i| i.size).sum();
    progress.begin(items.len(), listed);
    let result = run(
        items,
        options,
        transport,
        progress,
        cancel,
        log,
        stop_on_error,
    );
    progress.end();
    result
}

fn run(
    items: &[Item],
    options: &Options,
    transport: &dyn Transport,
    progress: &Progress,
    cancel: &AtomicBool,
    log: &dyn Fn(String),
    stop_on_error: bool,
) -> Result<(), Error> {
    let jobs = options.jobs.clamp(1, items.len().max(1));
    // What is missing, found in parallel; what is whole counts as done.
    let whole: Vec<AtomicBool> = items.iter().map(|_| AtomicBool::new(false)).collect();
    let next = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..jobs {
            scope.spawn(|| loop {
                let n = next.fetch_add(1, Relaxed);
                let Some(item) = items.get(n) else { break };
                if sha1_file(&item.path).as_ref() == Some(&item.sha1) {
                    whole[n].store(true, Relaxed);
                }
            });
        }
    });
    let mut need = 0u64;
    let mut missing = Vec::new();
    for (n, item) in items.iter().enumerate() {
        if whole[n].load(Relaxed) {
            progress.skipped(item.size.unwrap_or(0));
            progress.file_done();
        } else {
            let have = partial_len(&item.path);
            progress.skipped(have);
            need += item.size.unwrap_or(0).saturating_sub(have);
            missing.push(n);
        }
    }
    if let (Some(first), true) = (missing.first(), need > 0) {
        if let Some(free) = (options.free_space)(&items[*first].path) {
            if free < need.saturating_add(options.reserve) {
                return Err(Error::NoSpace {
                    need: need.saturating_add(options.reserve),
                    free,
                });
            }
        }
    }
    let next = AtomicUsize::new(0);
    let errors = AtomicUsize::new(0);
    let failed: Mutex<Option<Error>> = Mutex::new(None);
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..jobs.min(missing.len().max(1)))
            .map(|_| {
                scope.spawn(|| {
                    while let Some(&n) = missing.get(next.fetch_add(1, Relaxed)) {
                        if stop_on_error && failed.lock().unwrap().is_some() {
                            break;
                        }
                        if cancel.load(Relaxed) {
                            failed.lock().unwrap().get_or_insert(Error::Cancelled);
                            break;
                        }
                        match fetch_one(&items[n], options, transport, progress, cancel) {
                            Ok(()) => progress.file_done(),
                            Err(e) => {
                                errors.fetch_add(1, Relaxed);
                                let stop = matches!(e, Error::Cancelled | Error::NoSpace { .. });
                                failed.lock().unwrap().get_or_insert(e);
                                if stop {
                                    break;
                                }
                                progress.file_done();
                            }
                        }
                    }
                })
            })
            .collect();
        let mut waited = 0;
        while !workers.iter().all(|w| w.is_finished()) {
            std::thread::sleep(Duration::from_millis(100));
            waited += 1;
            if waited % 10 == 0 {
                if let Some(s) = progress.snapshot() {
                    log(format!("{} of {} files", s.files_done, s.files_total));
                }
            }
        }
    });
    match failed.into_inner().unwrap() {
        Some(e @ (Error::Cancelled | Error::NoSpace { .. })) => Err(e),
        Some(e) if !stop_on_error => Err(Error::Other(format!(
            "{} of {} files failed, the first: {e}",
            errors.load(Relaxed),
            items.len()
        ))),
        Some(e) => Err(e),
        None => Ok(()),
    }
}

fn io_error(path: &Path, e: &std::io::Error) -> Error {
    if e.kind() == std::io::ErrorKind::StorageFull || e.raw_os_error() == Some(28) {
        return Error::NoSpace {
            need: 0,
            free: free_space(path).unwrap_or(0),
        };
    }
    Error::Io(format!("{}: {e}", path.display()))
}

/// One file: continues its `.partial` with a Range request, checks size and sha1, and renames it
/// into place. A connection that fails is tried again from where it stopped; a refusal is not.
fn fetch_one(
    item: &Item,
    options: &Options,
    transport: &dyn Transport,
    progress: &Progress,
    cancel: &AtomicBool,
) -> Result<(), Error> {
    let partial = partial_path(&item.path);
    if let Some(dir) = item.path.parent() {
        fs::create_dir_all(dir).map_err(|e| io_error(dir, &e))?;
    }
    let mut last = None;
    // A resumed file that fails its sha1 is fetched once more from the start.
    let mut from_scratch = false;
    let mut tries = options.retries + 1;
    while tries > 0 {
        tries -= 1;
        if cancel.load(Relaxed) {
            return Err(Error::Cancelled);
        }
        let have = if from_scratch {
            0
        } else {
            partial_len(&item.path)
        };
        let from = match item.size {
            Some(size) if have >= size => 0,
            _ => have,
        };
        let body = match transport.open(&item.url, from) {
            Ok(body) => body,
            Err(Error::Status { status, .. }) if status < 500 => {
                return Err(Error::Status {
                    url: item.url.clone(),
                    status,
                })
            }
            Err(e) => {
                last = Some(e);
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
        };
        let append = from > 0 && body.partial;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .append(append)
            .truncate(!append)
            .open(&partial)
            .map_err(|e| io_error(&partial, &e))?;
        let mut reader = body.reader;
        let mut buffer = vec![0u8; 64 << 10];
        let mut broke = None;
        loop {
            if cancel.load(Relaxed) {
                return Err(Error::Cancelled);
            }
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    file.write_all(&buffer[..n])
                        .map_err(|e| io_error(&partial, &e))?;
                    progress.fetched(n as u64);
                }
                Err(e) => {
                    broke = Some(Error::Network {
                        url: item.url.clone(),
                        message: format!("stopped ({e})"),
                    });
                    break;
                }
            }
        }
        file.flush().map_err(|e| io_error(&partial, &e))?;
        drop(file);
        let size = partial_len(&item.path);
        if let Some(e) = broke {
            last = Some(e);
            continue;
        }
        if let Some(listed) = item.size.filter(|&listed| listed != size) {
            let _ = fs::remove_file(&partial);
            if from > 0 && !from_scratch {
                from_scratch = true;
                tries += 1;
                continue;
            }
            return Err(Error::BadSize {
                url: item.url.clone(),
                actual: size,
                listed,
            });
        }
        match sha1_file(&partial) {
            Some(actual) if actual == item.sha1 => {
                return fs::rename(&partial, &item.path).map_err(|e| io_error(&item.path, &e));
            }
            actual => {
                let _ = fs::remove_file(&partial);
                if from > 0 && !from_scratch {
                    from_scratch = true;
                    tries += 1;
                    continue;
                }
                return Err(Error::BadHash {
                    url: item.url.clone(),
                    actual: actual.unwrap_or_default(),
                });
            }
        }
    }
    Err(last.unwrap_or_else(|| Error::Other(format!("{} could not be downloaded", item.url))))
}
