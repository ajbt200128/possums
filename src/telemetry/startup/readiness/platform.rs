//! Best-effort local presentation. Never acquire the global stderr lock.
use std::{fmt, io::Write};

pub(super) fn emit(record: fmt::Arguments<'_>) {
    // Closed records fit below Linux PIPE_BUF (4096). Format fully before one
    // atomic nonblocking write; unavailable/full output is dropped, not retried.
    let mut buffer = [0; 1024];
    let mut line = std::io::Cursor::new(buffer.as_mut_slice());
    if line.write_fmt(record).is_err() || line.write_all(b"\n").is_err() {
        return;
    }
    let length = line.position() as usize;
    if let Some(mut output) = output() {
        let _ = output.write(&buffer[..length]);
    }
}

#[cfg(all(test, unix))]
mod tests;

#[cfg(target_os = "linux")]
fn output() -> Option<std::fs::File> {
    use std::os::unix::{fs::FileTypeExt, fs::OpenOptionsExt};
    const STDERR: &str = "/proc/self/fd/2";
    // Only Linux procfs reopening creates an independent open-file description.
    // In particular, do not dup/fcntl stderr or assume Darwin /dev/fd does this.
    // Regular files/TTYs do not guarantee nonblocking writes: fail closed.
    if !std::fs::metadata(STDERR).ok()?.file_type().is_fifo() {
        return None;
    }
    let output = std::fs::OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY)
        .open(STDERR)
        .ok()?;
    output
        .metadata()
        .ok()?
        .file_type()
        .is_fifo()
        .then_some(output)
}

#[cfg(not(target_os = "linux"))]
fn output() -> Option<std::fs::File> {
    None
}
