//! Writing to stdout / stderr.
//!
//! When a stream is a console, text is written with `WriteConsoleW` so that it
//! is displayed correctly regardless of the console code page. Otherwise raw
//! bytes are written.

use std::io::{self, Write};
use windows_sys::Win32::System::Console::{
    GetConsoleMode, GetStdHandle, STD_ERROR_HANDLE, STD_HANDLE, STD_OUTPUT_HANDLE, WriteConsoleW,
};

#[derive(Clone, Copy)]
pub enum Stream {
    Stdout,
    Stderr,
}

impl Stream {
    fn std_handle(self) -> STD_HANDLE {
        match self {
            Stream::Stdout => STD_OUTPUT_HANDLE,
            Stream::Stderr => STD_ERROR_HANDLE,
        }
    }
}

pub fn is_console(stream: Stream) -> bool {
    let mut mode = 0;
    // SAFETY: GetConsoleMode only writes to `mode`.
    unsafe {
        let handle = GetStdHandle(stream.std_handle());
        !handle.is_null() && GetConsoleMode(handle, &mut mode) != 0
    }
}

/// Writes text: as UTF-16 to a console, as UTF-8 otherwise.
pub fn write_text(stream: Stream, text: &str) -> io::Result<()> {
    if is_console(stream) {
        write_console(stream, text)
    } else {
        write_bytes(stream, text.as_bytes())
    }
}

pub fn write_bytes(stream: Stream, bytes: &[u8]) -> io::Result<()> {
    match stream {
        Stream::Stdout => {
            let mut out = io::stdout().lock();
            out.write_all(bytes)?;
            out.flush()
        }
        Stream::Stderr => io::stderr().lock().write_all(bytes),
    }
}

fn write_console(stream: Stream, text: &str) -> io::Result<()> {
    const CHUNK: usize = 8192;
    let wide: Vec<u16> = text.encode_utf16().collect();
    // SAFETY: GetStdHandle has no preconditions.
    let handle = unsafe { GetStdHandle(stream.std_handle()) };
    let mut rest = &wide[..];
    while !rest.is_empty() {
        let mut n = rest.len().min(CHUNK);
        // Do not split a surrogate pair.
        if n < rest.len() && (0xD800..0xDC00).contains(&rest[n - 1]) {
            n -= 1;
        }
        let mut written = 0u32;
        // SAFETY: `rest[..n]` is a valid buffer of `n` UTF-16 units.
        let ok = unsafe {
            WriteConsoleW(
                handle,
                rest.as_ptr(),
                n as u32,
                &mut written,
                std::ptr::null(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        if written == 0 {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "console write failed",
            ));
        }
        rest = &rest[written as usize..];
    }
    Ok(())
}
