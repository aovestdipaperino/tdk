// Copyright (c) 2026 Enzo Lombardi
// SPDX-License-Identifier: MIT

//! Cheap liveness marker for a running console.
//!
//! The console holds an exclusive advisory lock (`flock(2)`) on a well-known
//! file in the temp directory for its whole lifetime. The kernel releases the
//! lock the instant the process exits or crashes, so there is nothing to clean
//! up and no stale state after a crash -- unlike a PID file, which survives a
//! crash and lies.
//!
//! [`is_running`] tests that lock with a non-blocking *shared* lock request:
//! it fails with `EWOULDBLOCK` exactly when a console holds the exclusive
//! lock. It is two syscalls (`open` + `flock`) and never touches the network
//! stack, which is the point -- an app that only wants to know "is the console
//! up?" can call the library instead of opening a TCP connection to the
//! control port.
//!
//! Multiple callers may run [`is_running`] concurrently: shared locks do not
//! exclude each other, only the console's exclusive lock. The one race -- a
//! caller holding the shared lock at the instant the console tries to start --
//! is handled by [`acquire`]'s short retry, so a transient check cannot make
//! the console refuse to come up.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::Duration;

use libc::{LOCK_EX, LOCK_NB, LOCK_SH};

/// The well-known file the console locks. Lives in the temp directory; it is
/// not a config file -- it holds no settings, just an advisory lock, and the
/// OS creates or removes it as needed.
const LOCK_NAME: &str = "tdk.lock";

/// How many times [`acquire`] retries the exclusive lock before giving up, and
/// how long it sleeps between tries. A concurrent [`is_running`] check holds a
/// shared lock for one syscall, so a single short backoff is enough to let it
/// go; the retry exists only to ride out that microsecond window, not to wait
/// out another console (an exclusive lock held by another console never
/// clears).
const ACQUIRE_ATTEMPTS: u32 = 5;
const ACQUIRE_BACKOFF: Duration = Duration::from_millis(2);

/// The path the console locks.
fn lock_path() -> PathBuf {
    std::env::temp_dir().join(LOCK_NAME)
}

/// Opens (creating if absent) the lock file read/write, for the exclusive
/// holder.
fn open_or_create(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
}

/// Requests a non-blocking `flock` on `fd`. Returns `true` on success.
///
/// # Safety
/// `fd` must be a valid open file descriptor. The caller always passes the fd
/// of a `File` it owns, so this holds.
unsafe fn try_lock(fd: i32, operation: i32) -> bool {
    // SAFETY: `flock` takes an fd and an operation bitmask and performs no
    // memory I/O; the fd is valid and owned by the caller's `File`.
    unsafe { libc::flock(fd, operation) == 0 }
}

/// Acquires the exclusive lock a running console holds for its lifetime.
///
/// Returns the [`File`] that holds the lock; dropping it (or the process
/// exiting) releases it. Returns `None` if the lock cannot be acquired --
/// another console holds it, or the temp dir is unusable. The caller should
/// treat `None` as "best-effort marker unavailable" and fall back to whatever
/// authoritative check it already had (here, binding the control port), not
/// as a hard error.
///
/// A concurrent [`is_running`] check can briefly hold a shared lock that blocks
/// this exclusive request; this retries a few times so a transient check
/// cannot make the console refuse to come up.
#[must_use]
pub fn acquire() -> Option<File> {
    acquire_at(&lock_path())
}

/// Is a console currently running?
///
/// Cheaper than connecting to the control port: two syscalls (`open` +
/// `flock`), no network stack. The console holds an exclusive lock; this
/// requests a non-blocking shared lock, which fails with `EWOULDBLOCK`
/// exactly when the console is up. Multiple callers may check concurrently
/// (shared locks do not exclude each other).
///
/// Returns `false` when no console has ever run (the lock file does not exist)
/// or when the temp dir is unusable -- both mean "no console we can see."
#[must_use]
pub fn is_running() -> bool {
    is_running_at(&lock_path())
}

fn acquire_at(path: &Path) -> Option<File> {
    let file = open_or_create(path).ok()?;
    let fd = file.as_raw_fd();
    for attempt in 0..ACQUIRE_ATTEMPTS {
        // SAFETY: `fd` is the valid fd of `file`, which is owned for the
        // lifetime of this call.
        if unsafe { try_lock(fd, LOCK_EX | LOCK_NB) } {
            return Some(file);
        }
        if attempt + 1 < ACQUIRE_ATTEMPTS {
            std::thread::sleep(ACQUIRE_BACKOFF);
        }
    }
    None
}

fn is_running_at(path: &Path) -> bool {
    // Read-only, no create: a check must never create the file, only the
    // console does. If the file is absent, no console has ever run.
    let Ok(file) = File::open(path) else {
        return false;
    };
    // A shared, non-blocking request. Succeeds when no exclusive lock is held
    // (no console up); fails with EWOULDBLOCK when the console's exclusive
    // lock is in place.
    // SAFETY: `fd` is the valid fd of `file`, owned for this call.
    !unsafe { try_lock(file.as_raw_fd(), LOCK_SH | LOCK_NB) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A lock path unique to this test process, so the test never collides
    /// with a real console or a parallel test binary.
    fn test_path() -> PathBuf {
        std::env::temp_dir().join(format!("tdk-test-{}-{}.lock", std::process::id(), line!()))
    }

    #[test]
    fn acquire_makes_is_running_true_and_release_makes_it_false() {
        let path = test_path();
        let _ = std::fs::remove_file(&path);

        let lock = acquire_at(&path).expect("acquire should succeed with no holder");
        assert!(
            is_running_at(&path),
            "is_running must be true while the exclusive lock is held"
        );

        drop(lock);
        assert!(
            !is_running_at(&path),
            "is_running must be false once the lock is released"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn is_running_is_false_when_no_file_exists() {
        let path = test_path();
        let _ = std::fs::remove_file(&path);
        assert!(
            !is_running_at(&path),
            "is_running must be false when the lock file does not exist"
        );
    }
}
