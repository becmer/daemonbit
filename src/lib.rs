// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) $year Kamil Becmer

use std::{
    borrow::Cow,
    fmt, io,
    path::{Path, PathBuf},
};

use daemonbit_core::{Acquire, DaemonScope, ScopeKey, Scoped, TryAcquire};

#[cfg(unix)]
pub struct DaemonLock<T> {
    inner: daemonbit_lockfile::Lockfile<T>,
}
#[cfg(unix)]
impl<T: DaemonScope> Acquire<T> for DaemonLock<T> {
    fn acquire() -> Self {
        Self {
            inner: daemonbit_lockfile::Lockfile::acquire(),
        }
    }
}
#[cfg(unix)]
impl<T: DaemonScope> TryAcquire<T> for DaemonLock<T> {
    type Error = DaemonLockError;
    fn try_acquire() -> Result<Self, Self::Error> {
        Ok(Self {
            inner: daemonbit_lockfile::Lockfile::try_acquire()?,
        })
    }
}
#[cfg(unix)]
impl<T: DaemonScope> DaemonLock<T> {
    pub fn pid(&self) -> u32 {
        self.inner.pid()
    }
    pub fn path(&self) -> &Path {
        self.inner.path()
    }
}

#[cfg(windows)]
pub struct DaemonLock<T> {
    inner: daemonbit_winmutex::Mutex<T>,
    pid: u32,
}
#[cfg(windows)]
impl<T: DaemonScope> Acquire<T> for DaemonLock<T> {
    fn acquire() -> Self {
        Self {
            inner: daemonbit_winmutex::Mutex::acquire(),
            pid: std::process::id(),
        }
    }
}
#[cfg(windows)]
impl<T: DaemonScope> TryAcquire<T> for DaemonLock<T> {
    type Error = DaemonLockError;
    fn try_acquire() -> Result<Self, Self::Error> {
        Ok(Self {
            inner: daemonbit_winmutex::Mutex::try_acquire()?,
            pid: std::process::id(),
        })
    }
}
#[cfg(windows)]
impl<T: DaemonScope> DaemonLock<T> {
    pub fn pid(&self) -> u32 {
        self.pid
    }
    pub fn path(&self) -> &Path {
        Path::new(self.inner.name())
    }
}

impl<T: DaemonScope> Scoped for DaemonLock<T> {
    fn scope(&self) -> ScopeKey {
        T::key()
    }
}

#[derive(Debug)]
pub enum DaemonLockError {
    AccessFailed {
        scope: ScopeKey,
        path: PathBuf,
        source: io::Error,
    },
    AlreadyLocked {
        scope: ScopeKey,
        path: PathBuf,
        pid: Option<u32>,
    },
}
#[cfg(unix)]
impl From<daemonbit_lockfile::LockfileError> for DaemonLockError {
    fn from(e: daemonbit_lockfile::LockfileError) -> Self {
        use daemonbit_lockfile::LockfileError::*;
        match e {
            AccessFailed {
                scope,
                path,
                source,
            } => DaemonLockError::AccessFailed {
                scope,
                path,
                source,
            },
            AlreadyLocked { scope, path, pid } => {
                DaemonLockError::AlreadyLocked { scope, path, pid }
            }
        }
    }
}
#[cfg(windows)]
impl From<daemonbit_winmutex::MutexError> for DaemonLockError {
    fn from(e: daemonbit_winmutex::MutexError) -> Self {
        use daemonbit_winmutex::MutexError::*;
        match e {
            AccessFailed {
                scope,
                name,
                source,
            } => DaemonLockError::AccessFailed {
                scope,
                path: PathBuf::from(name),
                source,
            },
            AlreadyLocked { scope, name } => DaemonLockError::AlreadyLocked {
                scope,
                path: PathBuf::from(name),
                pid: None,
            },
        }
    }
}

impl fmt::Display for DaemonLockError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Self::AccessFailed {
                scope,
                ref path,
                ref source,
            } => {
                write!(
                    f,
                    "access failed: {scope}: {path}: {source}",
                    path = path.display()
                )
            }
            Self::AlreadyLocked {
                scope,
                ref path,
                pid,
            } => {
                let pid = match pid {
                    Some(pid) => Cow::Owned(pid.to_string()),
                    None => Cow::Borrowed("unknown"),
                };
                write!(
                    f,
                    "already locked: {scope}: {path}: {pid}",
                    path = path.display()
                )
            }
        }
    }
}
impl std::error::Error for DaemonLockError {}
