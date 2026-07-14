// crates/qbzd/src/lock.rs
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::io::AsRawFd;
use std::path::Path;

#[derive(Debug)]
pub struct InstanceLock {
    _file: std::fs::File,
}

#[derive(Debug)]
pub enum LockError {
    AlreadyRunning(Option<u32>),
    Io(String),
}

impl InstanceLock {
    /// flock on <data_root>/qbzd.lock, taken BEFORE the port bind (01 §8.1-4).
    /// Two daemons on one root = one device_uuid presented twice + two session.db
    /// writers — the lock is what protects those invariants, not the port.
    pub fn acquire(data_root: &Path) -> Result<Self, LockError> {
        std::fs::create_dir_all(data_root).map_err(|e| LockError::Io(e.to_string()))?;
        let path = data_root.join("qbzd.lock");
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| LockError::Io(e.to_string()))?;
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc != 0 {
            let mut pid = String::new();
            let _ = file.read_to_string(&mut pid);
            return Err(LockError::AlreadyRunning(pid.trim().parse().ok()));
        }
        file.set_len(0)
            .and_then(|_| file.seek(SeekFrom::Start(0)).map(|_| ()))
            .and_then(|_| write!(file, "{}", std::process::id()))
            .map_err(|e| LockError::Io(e.to_string()))?;
        Ok(Self { _file: file })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn second_acquire_fails_with_pid() {
        let dir = tempfile::tempdir().unwrap();
        let _l1 = InstanceLock::acquire(dir.path()).unwrap();
        // flock is per-open-file-description: a second open in the SAME process
        // still conflicts, which is exactly what we need to test.
        match InstanceLock::acquire(dir.path()) {
            Err(LockError::AlreadyRunning(pid)) => assert_eq!(pid, Some(std::process::id())),
            other => panic!("expected AlreadyRunning, got {other:?}"),
        }
    }
    #[test]
    fn released_lock_reacquires() {
        let dir = tempfile::tempdir().unwrap();
        drop(InstanceLock::acquire(dir.path()).unwrap());
        assert!(InstanceLock::acquire(dir.path()).is_ok());
    }
}
