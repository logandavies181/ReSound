use std::{
    error::Error,
    io::{self, Read, Write},
    sync::Mutex,
};

use gag::BufferRedirect;

static CAPTURE_LOCK: Mutex<()> = Mutex::new(());

pub fn capture<T>(
    operation: impl FnOnce() -> Result<T, Box<dyn Error>>,
) -> Result<(T, Vec<u8>, Vec<u8>), Box<dyn Error>> {
    let _lock = CAPTURE_LOCK
        .lock()
        .map_err(|_| io::Error::other("plugin output capture lock was poisoned"))?;
    let mut stdout = BufferRedirect::stdout()?;
    let mut stderr = BufferRedirect::stderr()?;
    let result = operation();

    io::stdout().flush()?;
    io::stderr().flush()?;
    if unsafe { libc::fflush(std::ptr::null_mut()) } == libc::EOF {
        return Err(io::Error::last_os_error().into());
    }

    let mut captured_stdout = Vec::new();
    let mut captured_stderr = Vec::new();
    stdout.read_to_end(&mut captured_stdout)?;
    stderr.read_to_end(&mut captured_stderr)?;
    drop(stderr);
    drop(stdout);

    Ok((result?, captured_stdout, captured_stderr))
}

#[cfg(test)]
mod tests {
    use super::capture;

    #[test]
    fn captures_stdout_and_stderr() {
        let (value, stdout, stderr) = capture(|| {
            let stdout_message = b"plugin stdout\n";
            let stderr_message = b"plugin stderr\n";
            unsafe {
                libc::write(
                    libc::STDOUT_FILENO,
                    stdout_message.as_ptr().cast(),
                    stdout_message.len(),
                );
                libc::write(
                    libc::STDERR_FILENO,
                    stderr_message.as_ptr().cast(),
                    stderr_message.len(),
                );
            }
            Ok(42)
        })
        .unwrap();

        assert_eq!(value, 42);
        assert_eq!(stdout, b"plugin stdout\n");
        assert_eq!(stderr, b"plugin stderr\n");
    }
}
