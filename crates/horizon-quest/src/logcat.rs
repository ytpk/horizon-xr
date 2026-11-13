//! Standard output and error into logcat: an Android app's are /dev/null, and
//! the game's logger prints Horizon's own messages to stderr.
use std::ffi::CString;
use std::io::{BufRead, BufReader};
use std::os::fd::FromRawFd;

pub const INFO: i32 = 4;
pub const WARN: i32 = 5;
pub const ERROR: i32 = 6;
const TAG: &std::ffi::CStr = c"horizon";

#[link(name = "log")]
unsafe extern "C" {
    fn __android_log_write(prio: i32, tag: *const libc::c_char, text: *const libc::c_char) -> i32;
}

/// One line to logcat under `horizon`.
pub fn write(priority: i32, line: &str) {
    let Ok(text) = CString::new(line.replace('\0', " ")) else {
        return;
    };
    // Safety: both are NUL-terminated strings that live through the call.
    unsafe {
        __android_log_write(priority, TAG.as_ptr(), text.as_ptr());
    }
}

/// Points fds 1 and 2 at a pipe whose lines a thread writes to logcat.
pub fn redirect_stdio() {
    let mut fds = [0; 2];
    // Safety: plain libc calls on descriptors this function owns.
    unsafe {
        if libc::pipe(fds.as_mut_ptr()) != 0 {
            return;
        }
        libc::dup2(fds[1], libc::STDOUT_FILENO);
        libc::dup2(fds[1], libc::STDERR_FILENO);
        libc::close(fds[1]);
    }
    // Safety: the pipe's read end, owned by the thread from here on.
    let read = unsafe { std::fs::File::from_raw_fd(fds[0]) };
    let _ = std::thread::Builder::new()
        .name("logcat".into())
        .spawn(move || {
            let mut lines = BufReader::new(read);
            let mut line = String::new();
            loop {
                line.clear();
                match lines.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let text = line.trim_end();
                        let priority = if text.contains("ERROR") || text.contains("panicked") {
                            ERROR
                        } else {
                            INFO
                        };
                        write(priority, text);
                    }
                }
            }
        });
}
