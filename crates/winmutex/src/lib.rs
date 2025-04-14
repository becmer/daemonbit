// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) $year Kamil Becmer

//noinspection RsCompileErrorMacro
#[cfg(all(not(windows), not(test)))]
mod api {
    compile_error!("unsupported platform");
}

#[cfg(all(windows, not(test)))]
mod api {
    use std::{
        ffi::{OsStr, OsString},
        io,
    };

    use daemonbit_core::DaemonScope;
    use widestring::U16CString;
    pub use windows::Win32::Foundation::HANDLE;
    use windows::{
        Win32::{
            Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError},
            System::Threading::CreateMutexW,
        },
        core::PCWSTR,
    };

    use super::{MutexError, ScopeExt};

    pub fn lock_exclusively<T: DaemonScope, S: AsRef<OsStr>>(
        name: S,
    ) -> Result<(OsString, HANDLE), MutexError> {
        let name = name.as_ref().to_os_string();
        let prefixed = T::key().with_prefix(&name);
        let Ok(prefixed) = U16CString::from_os_str(&prefixed) else {
            let scope = T::key();
            let source = io::Error::new(io::ErrorKind::InvalidInput, "invalid name");
            return Err(MutexError::AccessFailed {
                scope,
                name,
                source,
            });
        };
        let prefixed = PCWSTR(prefixed.as_ptr());
        let result = unsafe { CreateMutexW(None, true, prefixed) };
        if ERROR_ALREADY_EXISTS == unsafe { GetLastError() } {
            let scope = T::key();
            return Err(MutexError::AlreadyLocked { scope, name });
        }
        match result {
            Ok(handle) => Ok((name, handle)),
            Err(source) => {
                let scope = T::key();
                let source = io::Error::from(source);
                Err(MutexError::AccessFailed {
                    scope,
                    name,
                    source,
                })
            }
        }
    }

    pub fn close_handle(handle: HANDLE) -> io::Result<()> {
        unsafe { CloseHandle(handle) }.map_err(io::Error::from)
    }
}

//noinspection RsUnresolvedPath,RsInherentImplDifferentCrate,RsInvalidFieldsInStructLiteral,RsNonExistentFieldAccess
#[cfg(test)]
mod api {
    use std::{
        cell::RefCell,
        ffi::{OsStr, OsString},
        io,
    };

    use daemonbit_core::DaemonScope;

    use super::{MutexError, ScopeExt};

    pub struct HANDLE;

    pub enum MutexErrorMock {
        AccessFailed { source: io::Error },
        AlreadyLocked,
    }

    thread_local! {
        static LOCK: RefCell<Option<MutexErrorMock>> = RefCell::new(None);
        static CLOSE: RefCell<Option<io::Error>> = RefCell::new(None);
    }
    pub fn mock(lock_err: Option<MutexErrorMock>, close_err: Option<io::Error>) {
        LOCK.set(lock_err);
        CLOSE.set(close_err);
    }
    pub fn lock_exclusively<T: DaemonScope, S: AsRef<OsStr>>(
        name: S,
    ) -> Result<(OsString, HANDLE), MutexError> {
        let name = name.as_ref().to_os_string();
        let _ = T::key().with_prefix(&name);
        match LOCK.take() {
            None => Ok((name, HANDLE)),
            Some(MutexErrorMock::AccessFailed { source }) => {
                let scope = T::key();
                Err(MutexError::AccessFailed {
                    scope,
                    name,
                    source,
                })
            }
            Some(MutexErrorMock::AlreadyLocked) => {
                let scope = T::key();
                Err(MutexError::AlreadyLocked { scope, name })
            }
        }
    }
    pub fn close_handle(_: HANDLE) -> io::Result<()> {
        match CLOSE.take() {
            None => Ok(()),
            Some(err) => Err(err),
        }
    }
}

use std::{
    ffi::{OsStr, OsString},
    fmt, io,
    marker::PhantomData,
    mem::ManuallyDrop,
};

use daemonbit_core::{Acquire, DaemonScope, Global, Local, ScopeKey, Scoped, TryAcquire};
use tracing::warn;

pub type GlobalMutex = Mutex<Global>;
pub type LocalMutex = Mutex<Local>;

pub struct Mutex<T> {
    name: OsString,
    handle: ManuallyDrop<api::HANDLE>,
    _scope: PhantomData<fn(T) -> T>,
}
impl<T: DaemonScope> Scoped for Mutex<T> {
    fn scope(&self) -> ScopeKey {
        T::key()
    }
}
impl<T: DaemonScope> Acquire<T> for Mutex<T> {
    fn acquire() -> Self {
        Self::acquire_with_name(daemonbit_core::daemon_name())
    }
}
impl<T: DaemonScope> TryAcquire<T> for Mutex<T> {
    type Error = MutexError;
    fn try_acquire() -> Result<Self, Self::Error> {
        Self::try_acquire_with_name(daemonbit_core::daemon_name())
    }
}

impl<T: DaemonScope> Mutex<T> {
    pub fn acquire_with_name<S: AsRef<OsStr>>(name: S) -> Self {
        match Self::try_acquire_with_name(name) {
            Ok(mutex) => mutex,
            Err(e) => panic!("{e}"),
        }
    }
    pub fn try_acquire_with_name<S: AsRef<OsStr>>(name: S) -> Result<Self, MutexError> {
        let (name, handle) = api::lock_exclusively::<T, _>(name)?;
        Ok(Self {
            name,
            handle: ManuallyDrop::new(handle),
            _scope: PhantomData,
        })
    }
    pub fn scope(&self) -> ScopeKey {
        T::key()
    }
    pub fn name(&self) -> &OsStr {
        &self.name
    }
}
impl<T: DaemonScope> fmt::Debug for Mutex<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Mutex")
            .field("scope", &self.scope())
            .field("name", &self.name)
            .finish()
    }
}
impl<T> Drop for Mutex<T> {
    fn drop(&mut self) {
        let handle = unsafe { ManuallyDrop::take(&mut self.handle) };
        if let Err(error) = api::close_handle(handle) {
            warn!(?error, "cannot close mutex");
        }
    }
}

#[derive(Debug)]
pub enum MutexError {
    AccessFailed {
        scope: ScopeKey,
        name: OsString,
        source: io::Error,
    },
    AlreadyLocked {
        scope: ScopeKey,
        name: OsString,
    },
}
impl Scoped for MutexError {
    fn scope(&self) -> ScopeKey {
        match *self {
            Self::AccessFailed { scope, .. } => scope,
            Self::AlreadyLocked { scope, .. } => scope,
        }
    }
}
impl MutexError {
    pub fn name(&self) -> &OsStr {
        use MutexError::*;
        let (AccessFailed { name, .. } | AlreadyLocked { name, .. }) = self;
        name
    }
    pub fn into_name(self) -> OsString {
        use MutexError::*;
        let (AccessFailed { name, .. } | AlreadyLocked { name, .. }) = self;
        name
    }
}
impl fmt::Display for MutexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::AccessFailed {
                scope,
                ref name,
                ref source,
            } => write!(
                f,
                r"access failed: {}\{}: {source}",
                scope.as_prefix(),
                name.to_string_lossy(),
            ),
            Self::AlreadyLocked { scope, ref name } => {
                write!(
                    f,
                    r"already locked: {}\{}",
                    scope.as_prefix(),
                    name.to_string_lossy(),
                )
            }
        }
    }
}
impl std::error::Error for MutexError {}

trait ScopeExt {
    fn as_prefix(&self) -> &str;
    fn with_prefix<S: AsRef<OsStr>>(&self, name: S) -> OsString;
}
impl ScopeExt for ScopeKey {
    fn as_prefix(&self) -> &str {
        match *self {
            Self::Global => "Global",
            Self::Local => "Local",
        }
    }
    fn with_prefix<S: AsRef<OsStr>>(&self, name: S) -> OsString {
        let prefix = self.as_prefix();
        let name = name.as_ref();
        let mut prefixed = OsString::with_capacity(prefix.len() + 1 + name.len());
        prefixed.push(prefix);
        prefixed.push(r"\");
        prefixed.push(name);
        prefixed
    }
}

#[derive(Debug)]
pub enum ScopedMutex {
    Global(GlobalMutex),
    Local(LocalMutex),
}
impl ScopedMutex {
    pub fn acquire<S: AsRef<OsStr>>(name: S, scope: ScopeKey) -> Self {
        match scope {
            ScopeKey::Global => Self::Global(Mutex::<Global>::acquire_with_name(name)),
            ScopeKey::Local => Self::Local(Mutex::<Local>::acquire_with_name(name)),
        }
    }
    pub fn try_acquire<S: AsRef<OsStr>>(name: S, scope: ScopeKey) -> Result<Self, MutexError> {
        Ok(match scope {
            ScopeKey::Global => Self::Global(Mutex::<Global>::try_acquire_with_name(name)?),
            ScopeKey::Local => Self::Local(Mutex::<Local>::try_acquire_with_name(name)?),
        })
    }
    pub fn scope(&self) -> ScopeKey {
        match *self {
            Self::Global(_) => ScopeKey::Global,
            Self::Local(_) => ScopeKey::Local,
        }
    }
    pub fn name(&self) -> &OsStr {
        match self {
            Self::Global(m) => m.name(),
            Self::Local(m) => m.name(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use claims::{assert_err, assert_matches, assert_ok};
    use ntest::{assert_panics, test_case};

    use super::*;

    const MUTEX_NAME: &str = "test";

    #[test_case("global", name = "try_acquire_succeeded_global")]
    #[test_case("local", name = "try_acquire_succeeded_local")]
    fn try_acquire_succeeded(scope: &str) {
        let scope = ScopeKey::from_str(scope).unwrap();
        api::mock(None, None);

        let result = ScopedMutex::try_acquire(MUTEX_NAME, scope);

        let mutex = assert_ok!(result);
        assert_eq!(mutex.scope(), scope);
        assert_eq!(mutex.name(), OsStr::new(MUTEX_NAME));
    }

    #[test_case("global", name = "try_acquire_already_locked_global")]
    #[test_case("local", name = "try_acquire_already_locked_local")]
    fn try_acquire_already_locked(scope: &str) {
        let scope = ScopeKey::from_str(scope).unwrap();
        api::mock(Some(api::MutexErrorMock::AlreadyLocked), None);

        let result = ScopedMutex::try_acquire(MUTEX_NAME, scope);

        let error = assert_err!(result);
        assert_matches!(error, MutexError::AlreadyLocked { .. });
        assert_eq!(error.scope(), scope);
        assert_eq!(error.name(), OsStr::new(MUTEX_NAME));
    }

    #[test_case("global", name = "try_acquire_access_failed_global")]
    #[test_case("local", name = "try_acquire_access_failed_local")]
    fn try_acquire_access_failed(scope: &str) {
        let scope = ScopeKey::from_str(scope).unwrap();
        api::mock(
            Some(api::MutexErrorMock::AccessFailed {
                source: io::Error::new(io::ErrorKind::Other, "something went wrong"),
            }),
            None,
        );

        let result = ScopedMutex::try_acquire(MUTEX_NAME, scope);

        let error = assert_err!(result);
        assert_matches!(error, MutexError::AccessFailed { .. });
        assert_eq!(error.scope(), scope);
        assert_eq!(error.name(), OsStr::new(MUTEX_NAME));
    }

    #[test_case("global", name = "acquire_panics_on_error_global")]
    #[test_case("local", name = "acquire_panics_on_error_local")]
    fn acquire_panics_on_error(scope: &str) {
        let scope = ScopeKey::from_str(scope).unwrap();
        api::mock(Some(api::MutexErrorMock::AlreadyLocked), None);

        assert_panics!({
            let _ = ScopedMutex::acquire(MUTEX_NAME, scope);
        });
    }

    #[test_case("global", name = "mutex_drop_with_close_error_does_not_panic_global")]
    #[test_case("local", name = "mutex_drop_with_close_error_does_not_panic_local")]
    fn mutex_drop_with_close_error_does_not_panic(scope: &str) {
        let scope = ScopeKey::from_str(scope).unwrap();
        api::mock(
            None,
            Some(io::Error::new(io::ErrorKind::Other, "something went wrong")),
        );

        let mutex = ScopedMutex::acquire(MUTEX_NAME, scope);
        drop(mutex);
    }
}
