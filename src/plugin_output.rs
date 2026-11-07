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

    match result {
        Ok(value) => Ok((value, captured_stdout, captured_stderr)),
        Err(error) => {
            if let Err(output_error) = replay_output(&captured_stdout, &captured_stderr) {
                return Err(io::Error::other(format!(
                    "{error}; failed to replay plugin output: {output_error}"
                ))
                .into());
            }
            Err(error)
        }
    }
}

fn replay_output(stdout: &[u8], stderr: &[u8]) -> io::Result<()> {
    let mut output = io::stdout().lock();
    let mut error = io::stderr().lock();
    replay_to(&mut output, &mut error, stdout, stderr)
}

fn replay_to(
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    captured_stdout: &[u8],
    captured_stderr: &[u8],
) -> io::Result<()> {
    stdout.write_all(captured_stdout)?;
    stdout.flush()?;
    stderr.write_all(captured_stderr)?;
    stderr.flush()
}

#[cfg(test)]
mod tests {
    use super::{capture, replay_to};

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

    #[test]
    fn replays_captured_output_to_matching_streams() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        replay_to(
            &mut stdout,
            &mut stderr,
            b"plugin stdout\n",
            b"plugin stderr\n",
        )
        .unwrap();

        assert_eq!(stdout, b"plugin stdout\n");
        assert_eq!(stderr, b"plugin stderr\n");
    }
}
