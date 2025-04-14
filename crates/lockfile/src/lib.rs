// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) $year Kamil Becmer

//noinspection RsCompileErrorMacro
#[cfg(all(not(unix), not(test)))]
mod api {
    compile_error!("unsupported platform");
}

#[cfg(all(unix, not(test)))]
mod api {
    use std::io;
    pub use std::{
        fs::{File, remove_file},
        process::id as pid,
    };

    pub use nix::errno::Errno;

    pub type Flock = nix::fcntl::Flock<File>;

    pub fn open<P: AsRef<std::path::Path>>(path: P) -> io::Result<File> {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)
    }
    pub fn lock_exclusive(file: File) -> Result<Flock, (File, Errno)> {
        Flock::lock(file, nix::fcntl::FlockArg::LockExclusiveNonblock)
    }
}

//noinspection RsUnresolvedPath,RsInherentImplDifferentCrate,RsInvalidFieldsInStructLiteral,RsNonExistentFieldAccess
#[cfg(test)]
mod api {
    use std::{
        cell::RefCell,
        io,
        ops::{Deref, DerefMut},
        path::Path,
    };

    pub struct File {
        data: io::Cursor<Vec<u8>>,
    }
    impl File {
        fn new(data: Vec<u8>) -> Self {
            Self {
                data: io::Cursor::new(data),
            }
        }
        pub fn set_len(&mut self, len: usize) -> io::Result<()> {
            self.data.get_mut().truncate(len);
            Ok(())
        }
        pub fn sync_all(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    impl Deref for File {
        type Target = io::Cursor<Vec<u8>>;
        fn deref(&self) -> &Self::Target {
            &self.data
        }
    }
    impl DerefMut for File {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.data
        }
    }

    pub struct Flock {
        file: File,
    }
    impl Deref for Flock {
        type Target = File;
        fn deref(&self) -> &Self::Target {
            &self.file
        }
    }
    impl DerefMut for Flock {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.file
        }
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    #[repr(i32)]
    #[non_exhaustive]
    pub enum Errno {
        EWOULDBLOCK = 11,
        EACCES = 13,
    }

    thread_local! {
        static PID: RefCell<u32> = RefCell::new(0);
        static OPEN: RefCell<io::Result<Vec<u8>>> = RefCell::new(Ok(Vec::new()));
        static LOCK: RefCell<Option<Errno>> = RefCell::new(None);
    }
    pub fn mock(current_pid: u32, lockfile_data: io::Result<Vec<u8>>, lock_errno: Option<Errno>) {
        PID.set(current_pid);
        OPEN.set(lockfile_data);
        LOCK.set(lock_errno);
    }
    pub fn pid() -> u32 {
        PID.with(|v| *v.borrow())
    }
    pub fn open<P: AsRef<Path>>(_: P) -> io::Result<File> {
        Ok(File::new(OPEN.replace(Ok(Vec::new()))?))
    }
    pub fn lock_exclusive(file: File) -> Result<Flock, (File, Errno)> {
        match LOCK.take() {
            None => Ok(Flock { file }),
            Some(errno) => Err((file, errno)),
        }
    }
    pub fn remove_file<P: AsRef<Path>>(_: P) -> io::Result<()> {
        Ok(())
    }
}

use std::{
    borrow::Cow,
    fmt, io,
    io::{Read, Seek, Write},
    marker::PhantomData,
    mem::ManuallyDrop,
    ops::Deref,
    path::{Path, PathBuf},
};

use daemonbit_core::{Acquire, DaemonScope, ScopeKey, Scoped, TryAcquire};
use daemonbit_rundir::{RuntimeDirectory, ScopedRuntimeDirectory};
use tracing::warn;

pub struct Lockfile<T> {
    raw: RawLockfile,
    _scope: PhantomData<fn(T) -> T>,
}
impl<T: DaemonScope> Scoped for Lockfile<T> {
    fn scope(&self) -> ScopeKey {
        T::key()
    }
}
impl<T: DaemonScope> Acquire<T> for Lockfile<T> {
    fn acquire() -> Self {
        match RuntimeDirectory::<T>::get() {
            Ok(rundir) => {
                let raw = RawLockfile::acquire_inner(rundir.join("lock"), Some(rundir.into()));
                Self {
                    raw,
                    _scope: PhantomData,
                }
            }
            Err((path, source)) => {
                let path = path.join("lock");
                panic!("{}", RawLockfileError::AccessFailed { path, source });
            }
        }
    }
}

impl<T: DaemonScope> TryAcquire<T> for Lockfile<T> {
    type Error = LockfileError;
    fn try_acquire() -> Result<Self, Self::Error> {
        match RuntimeDirectory::<T>::get() {
            Ok(rundir) => {
                match RawLockfile::try_acquire_inner(rundir.join("lock"), Some(rundir.into())) {
                    Ok(raw) => Ok(Self {
                        raw,
                        _scope: PhantomData,
                    }),
                    Err(e) => {
                        let scope = T::key();
                        Err(match e {
                            RawLockfileError::AccessFailed { path, source } => {
                                LockfileError::AccessFailed {
                                    scope,
                                    path,
                                    source,
                                }
                            }
                            RawLockfileError::AlreadyLocked { path, pid } => {
                                LockfileError::AlreadyLocked { scope, path, pid }
                            }
                        })
                    }
                }
            }
            Err((path, source)) => {
                let scope = T::key();
                let path = path.join("lock");
                Err(LockfileError::AccessFailed {
                    scope,
                    path,
                    source,
                })
            }
        }
    }
}
impl<T: DaemonScope> Deref for Lockfile<T> {
    type Target = RawLockfile;
    fn deref(&self) -> &Self::Target {
        &self.raw
    }
}

pub struct RawLockfile {
    pid: u32,
    path: PathBuf,
    flock: ManuallyDrop<api::Flock>,
    rundir: Option<ScopedRuntimeDirectory>,
}
impl RawLockfile {
    pub fn acquire_with_path<P: AsRef<Path>>(path: P) -> Self {
        Self::acquire_inner(path, None)
    }
    pub fn try_acquire_with_path<P: AsRef<Path>>(path: P) -> Result<Self, RawLockfileError> {
        Self::try_acquire_inner(path, None)
    }
    fn acquire_inner<P: AsRef<Path>>(path: P, rundir: Option<ScopedRuntimeDirectory>) -> Self {
        match Self::try_acquire_inner(path, rundir) {
            Ok(lockfile) => lockfile,
            Err(e) => panic!("{e}"),
        }
    }
    fn try_acquire_inner<P: AsRef<Path>>(
        path: P,
        rundir: Option<ScopedRuntimeDirectory>,
    ) -> Result<Self, RawLockfileError> {
        let path = path.as_ref().to_path_buf();

        let file = match api::open(&path) {
            Ok(file) => file,
            Err(source) => return Err(RawLockfileError::AccessFailed { path, source }),
        };

        match api::lock_exclusive(file) {
            Ok(mut flock) => {
                let pid = api::pid();
                let path = UnparsedPid::read(path, &mut *flock)?
                    .parse_checked(pid)
                    .write(pid, &mut *flock)?;
                let flock = ManuallyDrop::new(flock);
                Ok(RawLockfile {
                    pid,
                    path,
                    flock,
                    rundir,
                })
            }
            Err((mut file, api::Errno::EWOULDBLOCK)) => Err(UnparsedPid::read(path, &mut file)?
                .parse()
                .into_already_locked_error()),
            Err((_, errno)) => {
                #[allow(clippy::unnecessary_cast)]
                let source = io::Error::from_raw_os_error(errno as i32);
                Err(RawLockfileError::AccessFailed { path, source })
            }
        }
    }
    pub fn pid(&self) -> u32 {
        self.pid
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
}
impl fmt::Debug for RawLockfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Lockfile")
            .field("pid", &self.pid)
            .field("path", &self.path)
            .finish()
    }
}
impl Drop for RawLockfile {
    fn drop(&mut self) {
        if let Err(error) = api::remove_file(&self.path) {
            warn!(?error, "cannot remove lockfile")
        }
        unsafe { ManuallyDrop::drop(&mut self.flock) };
        self.rundir.take();
    }
}

#[derive(Debug)]
struct ParsedPid {
    path: PathBuf,
    pid: Option<u32>,
}
impl ParsedPid {
    fn parse(path: PathBuf, input: &[u8]) -> Result<Self, (PathBuf, io::Error)> {
        if input.is_empty() {
            return Ok(Self { path, pid: None });
        }

        let result = match input.iter().all(u8::is_ascii_digit) {
            true => Ok(input),
            false => Err(io::Error::new(io::ErrorKind::InvalidData, "digit expected")),
        }
        .and_then(|input| std::str::from_utf8(input).or_invalid_data())
        .and_then(|s| s.parse::<u32>().or_invalid_data());

        match result {
            Ok(pid) => Ok(Self {
                path,
                pid: Some(pid),
            }),
            Err(source) => Err((path, source)),
        }
    }
    fn write(self, pid: u32, output: &mut api::File) -> Result<PathBuf, RawLockfileError> {
        match output
            .rewind()
            .and_then(|_| output.set_len(0))
            .and_then(|_| {
                let pid = pid.to_string();
                output.write_all(pid.as_bytes())
            })
            .and_then(|_| output.sync_all())
        {
            Ok(_) => Ok(self.path),
            Err(source) => Err(RawLockfileError::AccessFailed {
                path: self.path,
                source,
            }),
        }
    }
    fn into_already_locked_error(self) -> RawLockfileError {
        RawLockfileError::AlreadyLocked {
            path: self.path,
            pid: self.pid,
        }
    }
}

struct UnparsedPid {
    path: PathBuf,
    data: Vec<u8>,
}
impl UnparsedPid {
    fn read(path: PathBuf, input: &mut api::File) -> Result<Self, RawLockfileError> {
        match input.rewind().and_then(|_| {
            let mut data = Vec::<u8>::with_capacity(10);
            input.read_to_end(&mut data).map(|_| data)
        }) {
            Ok(data) => Ok(Self { path, data }),
            Err(source) => Err(RawLockfileError::AccessFailed { path, source }),
        }
    }
    fn parse_checked(self, current_pid: u32) -> ParsedPid {
        match ParsedPid::parse(self.path, self.data.trim_ascii()) {
            Ok(parsed) => {
                match parsed.pid {
                    Some(written_pid) if written_pid != current_pid => {
                        warn!(written_pid, current_pid, lockfile = %parsed.path.display(), "encountered stale lockfile");
                    }
                    _ => {}
                }
                parsed
            }
            Err((path, error)) => {
                warn!(current_pid, lockfile = %path.display(), %error, "encountered invalid lockfile");
                ParsedPid { path, pid: None }
            }
        }
    }
    fn parse(self) -> ParsedPid {
        ParsedPid::parse(self.path, self.data.trim_ascii())
            .unwrap_or_else(|(path, _)| ParsedPid { path, pid: None })
    }
}

trait OrInvalidData: Sized {
    type Output;
    fn or_invalid_data(self) -> io::Result<Self::Output>;
}
impl<T, E> OrInvalidData for Result<T, E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    type Output = T;
    fn or_invalid_data(self) -> io::Result<T> {
        match self {
            Ok(v) => Ok(v),
            Err(e) => Err(io::Error::new(io::ErrorKind::InvalidData, e)),
        }
    }
}

#[derive(Debug)]
pub enum LockfileError {
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
impl Scoped for LockfileError {
    fn scope(&self) -> ScopeKey {
        match *self {
            Self::AccessFailed { scope, .. } => scope,
            Self::AlreadyLocked { scope, .. } => scope,
        }
    }
}
impl LockfileError {
    pub fn path(&self) -> &Path {
        use LockfileError::*;
        let (AccessFailed { path, .. } | AlreadyLocked { path, .. }) = self;
        path
    }
    pub fn into_path(self) -> PathBuf {
        use LockfileError::*;
        let (AccessFailed { path, .. } | AlreadyLocked { path, .. }) = self;
        path
    }
}
impl fmt::Display for LockfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::AccessFailed {
                ref path,
                ref source,
                ..
            } => write!(f, "access failed: {}: {source}", path.display()),
            Self::AlreadyLocked { ref path, pid, .. } => {
                let pid = match pid {
                    Some(pid) => Cow::Owned(pid.to_string()),
                    None => Cow::Borrowed("unknown"),
                };
                write!(f, "already locked: {}: {pid}", path.display())
            }
        }
    }
}
impl std::error::Error for LockfileError {}

#[derive(Debug)]
pub enum RawLockfileError {
    AccessFailed { path: PathBuf, source: io::Error },
    AlreadyLocked { path: PathBuf, pid: Option<u32> },
}
impl RawLockfileError {
    pub fn path(&self) -> &Path {
        use RawLockfileError::*;
        let (AccessFailed { path, .. } | AlreadyLocked { path, .. }) = self;
        path
    }
    pub fn into_path(self) -> PathBuf {
        use RawLockfileError::*;
        let (AccessFailed { path, .. } | AlreadyLocked { path, .. }) = self;
        path
    }
}
impl fmt::Display for RawLockfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::AccessFailed {
                ref path,
                ref source,
            } => write!(f, "access failed: {}: {source}", path.display()),
            Self::AlreadyLocked { ref path, pid } => {
                let pid = match pid {
                    Some(pid) => Cow::Owned(pid.to_string()),
                    None => Cow::Borrowed("unknown"),
                };
                write!(f, "already locked: {}: {pid}", path.display())
            }
        }
    }
}
impl std::error::Error for RawLockfileError {}

#[cfg(test)]
mod tests {
    use claims::assert_matches;
    use daemonbit_test::capture_tracing;

    use super::*;

    macro_rules! pid {
        ($pid:expr) => {{
            let pid = ($pid).to_string();
            pid.as_bytes().to_vec()
        }};
    }

    macro_rules! unparsed {
        ($pid:expr) => {
            UnparsedPid {
                path: PathBuf::new(),
                data: $pid,
            }
        };
    }

    macro_rules! parsed {
        ($pid:expr) => {
            ParsedPid {
                path: PathBuf::new(),
                pid: $pid,
            }
        };
    }

    const CURRENT_PID: u32 = 123;
    const FOREIGN_PID: u32 = 456;
    const INVALID_DATA: &[u8] = b"invalid";

    #[test]
    fn parse_empty() {
        let result = ParsedPid::parse(PathBuf::new(), b"");
        assert_matches!(result, Ok(ParsedPid { pid: None, .. }));
    }

    #[test]
    fn parse_invalid_utf8() {
        let result = ParsedPid::parse(PathBuf::new(), b"\xff");
        assert_matches!(result, Err((_, e)) if e.kind() == io::ErrorKind::InvalidData);
    }

    #[test]
    fn parse_garbage() {
        let result = ParsedPid::parse(PathBuf::new(), b"123 pid");
        assert_matches!(result, Err((_, e)) if e.kind() == io::ErrorKind::InvalidData);
    }

    #[test]
    fn parse_negative_number() {
        let result = ParsedPid::parse(PathBuf::new(), b"-123");
        assert_matches!(result, Err((_, e)) if e.kind() == io::ErrorKind::InvalidData);
    }

    #[test]
    fn parse_zero() {
        let result = ParsedPid::parse(PathBuf::new(), b"0");
        assert_matches!(result, Ok(ParsedPid { pid: Some(0), .. }));
    }

    #[test]
    fn parse_positive_number_signed() {
        let result = ParsedPid::parse(PathBuf::new(), b"+123");
        assert_matches!(result, Err((_, e)) if e.kind() == io::ErrorKind::InvalidData);
    }

    #[test]
    fn parse_positive_number_unsigned() {
        let result = ParsedPid::parse(PathBuf::new(), b"123");
        assert_matches!(result, Ok(ParsedPid { pid: Some(123), .. }));
    }

    #[test]
    fn try_parse_different_pid_warns_stale() {
        let parsed = capture_tracing(|| unparsed!(pid!(FOREIGN_PID)).parse_checked(CURRENT_PID));
        assert!(parsed.logged("encountered stale lockfile"));
        assert_matches!(parsed.pid, Some(FOREIGN_PID));
    }

    #[test]
    fn try_parse_same_pid_do_not_warn() {
        let parsed = capture_tracing(|| unparsed!(pid!(CURRENT_PID)).parse_checked(CURRENT_PID));
        assert!(!parsed.logged("encountered stale lockfile"));
        assert_matches!(parsed.pid, Some(CURRENT_PID));
    }

    #[test]
    fn try_parse_empty_do_not_warn() {
        let parsed = capture_tracing(|| unparsed!(Vec::new()).parse_checked(CURRENT_PID));
        assert!(!parsed.logged("encountered stale lockfile"));
        assert_matches!(parsed.pid, None);
    }

    #[test]
    fn try_parse_unparseable_pid_warns_invalid() {
        let parsed =
            capture_tracing(|| unparsed!(INVALID_DATA.to_vec()).parse_checked(CURRENT_PID));
        assert!(parsed.logged("encountered invalid lockfile"));
        assert_matches!(parsed.pid, None);
    }

    #[test]
    fn into_already_locked_error_with_different_pid() {
        let error = parsed!(Some(FOREIGN_PID)).into_already_locked_error();
        assert_matches!(
            error,
            RawLockfileError::AlreadyLocked {
                pid: Some(FOREIGN_PID),
                ..
            }
        );
    }

    #[test]
    fn into_already_locked_error_with_none() {
        let error = parsed!(None).into_already_locked_error();
        assert_matches!(error, RawLockfileError::AlreadyLocked { pid: None, .. });
    }

    #[test]
    fn lockfile_open_writes_pid() {
        api::mock(CURRENT_PID, Ok(pid!(FOREIGN_PID)), None);
        let lockfile = RawLockfile::try_acquire_with_path("").unwrap();
        assert_eq!(lockfile.pid, CURRENT_PID);
        assert_eq!(lockfile.flock.get_ref(), &pid!(CURRENT_PID));
    }

    #[test]
    fn lockfile_open_not_found() {
        api::mock(
            CURRENT_PID,
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "no such file or directory",
            )),
            None,
        );
        let result = RawLockfile::try_acquire_with_path("");
        assert_matches!(result, Err(RawLockfileError::AccessFailed { .. }));
    }

    #[test]
    fn lockfile_open_already_locked() {
        api::mock(
            CURRENT_PID,
            Ok(pid!(FOREIGN_PID)),
            Some(api::Errno::EWOULDBLOCK),
        );
        let result = RawLockfile::try_acquire_with_path("");
        assert_matches!(
            result,
            Err(RawLockfileError::AlreadyLocked {
                pid: Some(FOREIGN_PID),
                ..
            })
        );
    }

    #[test]
    fn lockfile_open_other_lock_error() {
        api::mock(CURRENT_PID, Ok(pid!(FOREIGN_PID)), Some(api::Errno::EACCES));
        let result = RawLockfile::try_acquire_with_path("");
        assert_matches!(result, Err(RawLockfileError::AccessFailed { .. }));
    }
}
