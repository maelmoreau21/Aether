//! The disposable extension worker process, shared by every Core caller.
//!
//! The Core owns how a guest is isolated: an empty environment, no stderr, a
//! process killed with its handle, and length-prefixed JSON frames whose size
//! is checked before any allocation. Callers own the protocol on top (which
//! frames they accept, which errors they report), so each keeps its own error
//! codes and this module never decides an HTTP status.
use std::process::Stdio;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{Child, ChildStdin, ChildStdout, Command},
};

/// Largest frame the worker may send: one SDK JSON payload plus its envelope.
pub(crate) const MAX_FRAME: usize = media_extension_sdk::MAX_JSON + 4096;

/// The worker binary sits next to the Core executable. In Cargo's tests it is in
/// the profile directory: one level up from `deps` (Rust ≤ 1.99), or four up from
/// `build/<package>/<hash>/out` (the build directory layout of Rust 1.100).
fn binary() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let mut directory = exe.parent()?;
    let named = |dir: &std::path::Path, name: &str| dir.file_name().is_some_and(|n| n == name);
    if named(directory, "deps") {
        directory = directory.parent()?;
    } else if named(directory, "out")
        && let Some(build) = directory.ancestors().nth(3)
        && named(build, "build")
    {
        directory = build.parent()?;
    }
    Some(directory.join(if cfg!(windows) {
        "media-extension-worker.exe"
    } else {
        "media-extension-worker"
    }))
}

/// Start one isolated worker. `None` means the worker is unavailable.
pub(crate) fn spawn() -> Option<Child> {
    let mut command = Command::new(binary()?);
    command
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    // CREATE_NO_WINDOW: a worker never opens a console window.
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    command.spawn().ok()
}

/// Write one big-endian length-prefixed frame.
pub(crate) async fn write_frame(stdin: &mut ChildStdin, bytes: &[u8]) -> std::io::Result<()> {
    let len = u32::try_from(bytes.len())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    stdin.write_all(&len.to_be_bytes()).await?;
    stdin.write_all(bytes).await
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum FrameError {
    /// The pipe closed or failed before a whole frame arrived.
    Io,
    /// The announced length exceeds [`MAX_FRAME`]; nothing was allocated.
    TooLarge,
}

/// Read one length-prefixed frame, refusing oversized lengths before allocating.
pub(crate) async fn read_frame(stdout: &mut ChildStdout) -> Result<Vec<u8>, FrameError> {
    read_bounded(stdout, MAX_FRAME).await
}

async fn read_bounded(
    reader: &mut (impl AsyncReadExt + Unpin),
    maximum: usize,
) -> Result<Vec<u8>, FrameError> {
    let mut header = [0; 4];
    reader
        .read_exact(&mut header)
        .await
        .map_err(|_| FrameError::Io)?;
    let len = u32::from_be_bytes(header) as usize;
    if len > maximum {
        return Err(FrameError::TooLarge);
    }
    let mut bytes = vec![0; len];
    reader
        .read_exact(&mut bytes)
        .await
        .map_err(|_| FrameError::Io)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn frames_are_bounded_before_allocation_and_truncation_is_an_io_error() {
        let oversized = ((MAX_FRAME + 1) as u32).to_be_bytes();
        assert_eq!(
            read_bounded(&mut &oversized[..], MAX_FRAME).await,
            Err(FrameError::TooLarge)
        );
        assert_eq!(
            read_bounded(&mut &[0, 0, 0, 3, b'{'][..], MAX_FRAME).await,
            Err(FrameError::Io)
        );
        assert_eq!(
            read_bounded(&mut &[0, 0][..], MAX_FRAME).await,
            Err(FrameError::Io)
        );
        let mut frame = 2u32.to_be_bytes().to_vec();
        frame.extend_from_slice(b"{}");
        assert_eq!(
            read_bounded(&mut &frame[..], MAX_FRAME).await,
            Ok(b"{}".to_vec())
        );
    }

    #[test]
    fn worker_binary_is_resolved_next_to_the_core() {
        let path = binary().expect("the test executable has a parent directory");
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("media-extension-worker"));
        assert_ne!(
            path.parent().and_then(|p| p.file_name()),
            Some(std::ffi::OsStr::new("deps"))
        );
    }
}
