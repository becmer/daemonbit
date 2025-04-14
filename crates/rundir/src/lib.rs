// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) $year Kamil Becmer

//noinspection RsCompileErrorMacro
//noinspection RsCompileErrorMacro
#[cfg(all(not(unix), not(test)))]
mod api {
    compile_error!("unsupported platform");
}

#[cfg(all(unix, not(test)))]
mod api {
    use std::{
        borrow::Cow,
        ffi::OsStr,
        io,
        path::{Path, PathBuf},
        sync::atomic::AtomicPtr,
    };

    use daemonbit_core::{DaemonScope, Global, Local, ScopeKey};

    use super::Shared;

    static GLOBAL_RUNDIR: AtomicPtr<Shared<Global>> = AtomicPtr::new(std::ptr::null_mut());
    static LOCAL_RUNDIR: AtomicPtr<Shared<Local>> = AtomicPtr::new(std::ptr::null_mut());

    pub fn atomic<'a, T: DaemonScope>() -> &'a AtomicPtr<Shared<T>> {
        let ptr = match T::key() {
            ScopeKey::Global => &raw const GLOBAL_RUNDIR as *const AtomicPtr<Shared<T>>,
            ScopeKey::Local => &raw const LOCAL_RUNDIR as *const AtomicPtr<Shared<T>>,
        };
        unsafe { &*ptr }
    }
    pub fn daemon_name() -> Cow<'static, OsStr> {
        Cow::Borrowed(OsStr::new(daemonbit_core::daemon_name()))
    }
    pub fn euid() -> u32 {
        nix::unistd::Uid::effective().as_raw()
    }
    pub fn run_dir() -> Option<&'static Path> {
        ["/run", "/var/run"]
            .into_iter()
            .map(Path::new)
            .find(|p| p.is_dir())
    }
    pub fn xdg_runtime_dir() -> Option<PathBuf> {
        std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from)
    }
    pub fn temp_dir() -> PathBuf {
        std::env::temp_dir()
    }
    pub fn is_dir<P: AsRef<Path>>(path: P) -> bool {
        path.as_ref().is_dir()
    }
    pub fn create_dir_all<P: AsRef<Path>>(path: P) -> io::Result<()> {
        std::fs::create_dir_all(path)
    }
    pub fn remove_dir_all<P: AsRef<Path>>(path: P) -> io::Result<()> {
        std::fs::remove_dir_all(path)
    }
}

//noinspection RsUnresolvedPath,RsInherentImplDifferentCrate,RsInvalidFieldsInStructLiteral,RsNonExistentFieldAccess,RsReferenceIsNotPublic,RsUnresolvedMethod
#[cfg(test)]
mod api {
    use std::{
        borrow::Cow,
        cell::RefCell,
        ffi::OsStr,
        io,
        path::{Path, PathBuf},
        sync::atomic::{AtomicPtr, Ordering::AcqRel},
    };

    use daemonbit_core::{DaemonScope, Global, Local, ScopeKey};

    use super::Shared;

    thread_local! {
        static GLOBAL_RUNDIR: RefCell<AtomicPtr<Shared<Global>>> = RefCell::new(AtomicPtr::default());
        static LOCAL_RUNDIR: RefCell<AtomicPtr<Shared<Local>>> = RefCell::new(AtomicPtr::default());
        static NAME: RefCell<&'static str> = RefCell::new("");
        static EUID: RefCell<u32> = RefCell::new(0);
        static RUN_DIR: RefCell<Option<&'static str>> = RefCell::new(None);
        static XDG_RUNTIME_DIR: RefCell<Option<&'static str>> = RefCell::new(None);
    }
    pub fn mock(
        name: &'static str,
        euid: u32,
        run_dir: Option<&'static str>,
        xdg_runtime_dir: Option<&'static str>,
    ) {
        reset::<Global>();
        reset::<Local>();
        NAME.set(name);
        EUID.set(euid);
        RUN_DIR.set(run_dir);
        XDG_RUNTIME_DIR.set(xdg_runtime_dir);
    }
    #[allow(clippy::unnecessary_cast)]
    pub fn atomic<'a, T: DaemonScope>() -> &'a AtomicPtr<Shared<T>> {
        let ptr = match T::key() {
            ScopeKey::Global => GLOBAL_RUNDIR.with_borrow(|v: &AtomicPtr<Shared<Global>>| {
                &raw const *v as *const AtomicPtr<Shared<T>>
            }),
            ScopeKey::Local => LOCAL_RUNDIR.with_borrow(|v: &AtomicPtr<Shared<Local>>| {
                &raw const *v as *const AtomicPtr<Shared<T>>
            }),
        };
        unsafe { &*ptr }
    }
    fn reset<T: DaemonScope>() {
        let _ = Shared::try_from_raw(atomic::<T>().swap(std::ptr::null_mut(), AcqRel));
    }
    pub fn daemon_name() -> Cow<'static, OsStr> {
        let name = NAME.with(|v| *v.borrow());
        Cow::Borrowed(OsStr::new(name))
    }
    pub fn euid() -> u32 {
        EUID.with(|v| *v.borrow())
    }
    pub fn run_dir() -> Option<&'static Path> {
        RUN_DIR.with_borrow(|v| *v).map(Path::new)
    }
    pub fn xdg_runtime_dir() -> Option<PathBuf> {
        XDG_RUNTIME_DIR.with_borrow(|v| *v).map(PathBuf::from)
    }
    pub fn temp_dir() -> PathBuf {
        PathBuf::from("/tmp")
    }
    pub fn is_dir<P: AsRef<Path>>(_: P) -> bool {
        true
    }
    pub fn create_dir_all<P: AsRef<Path>>(_: P) -> io::Result<()> {
        Ok(())
    }
    pub fn remove_dir_all<P: AsRef<Path>>(_: P) -> io::Result<()> {
        Ok(())
    }
}

use std::{
    borrow::{Borrow, Cow},
    cell::{OnceCell, UnsafeCell},
    ffi::{OsStr, OsString},
    io,
    marker::PhantomData,
    ops::Deref,
    path::{Path, PathBuf},
    sync::{
        Arc, Weak,
        atomic::{
            AtomicPtr,
            Ordering::{AcqRel, Acquire},
        },
    },
};

use daemonbit_core::{DaemonScope, Global, Local, ScopeKey};
use tracing::warn;

pub struct RuntimeDirectory<T> {
    shared: Arc<Shared<T>>,
}
impl<T: DaemonScope> RuntimeDirectory<T> {
    pub fn as_path(&self) -> &Path {
        &self.shared.path
    }
    pub fn temporary(&self) -> bool {
        self.shared.state.temporary()
    }
    pub fn get() -> Result<Self, (PathBuf, io::Error)> {
        let shared = OnceCell::new();
        let atomic: &AtomicPtr<Shared<T>> = api::atomic::<T>();
        let mut current = atomic.load(Acquire);

        loop {
            if let Some(current) = Shared::try_from_raw(current) {
                let _ = shared.into_inner();
                return Ok(Self { shared: current });
            }
            let new = shared
                .get_or_init(|| {
                    let shared = Shared::<T>::new(api::daemon_name());
                    let ptr = shared.to_raw();
                    (shared, ptr)
                })
                .1;
            if let Err(ptr) = atomic.compare_exchange(current, new, AcqRel, Acquire) {
                current = ptr;
                continue;
            }
            let shared = shared.into_inner().expect("already initialized").0;
            return match shared.state.init(&shared.path) {
                Ok(()) => Ok(Self { shared }),
                Err(e) => Err((shared.path.clone(), e)),
            };
        }
    }
}
impl<T: DaemonScope> AsRef<Path> for RuntimeDirectory<T> {
    fn as_ref(&self) -> &Path {
        &self.shared.path
    }
}
impl<T: DaemonScope> Borrow<Path> for RuntimeDirectory<T> {
    fn borrow(&self) -> &Path {
        &self.shared.path
    }
}
impl<T: DaemonScope> Deref for RuntimeDirectory<T> {
    type Target = Path;
    fn deref(&self) -> &Self::Target {
        &self.shared.path
    }
}

struct Shared<T> {
    path: PathBuf,
    state: State,
    _scope: PhantomData<fn(T) -> T>,
}
impl<T: DaemonScope> Shared<T> {
    fn new<S: AsRef<OsStr>>(name: S) -> Arc<Self> {
        let scope = T::key();
        let name = name.as_ref();
        let (path, state) = match scope.rundir_path() {
            Some(path) => (path.join(name), State::Persistent),
            None => {
                let path = api::temp_dir().join(scope.rundir_name(name));
                (path, State::Temporary)
            }
        };
        Arc::new(Self {
            path,
            state,
            _scope: PhantomData,
        })
    }
    fn to_raw(self: &Arc<Self>) -> *mut Self {
        Arc::downgrade(self).into_raw().cast_mut()
    }
    fn try_from_raw(ptr: *mut Self) -> Option<Arc<Self>> {
        if ptr.is_null() {
            None
        } else {
            unsafe { Weak::from_raw(ptr.cast_const()) }
                .clone()
                .upgrade()
        }
    }
}
impl<T> Drop for Shared<T> {
    fn drop(&mut self) {
        if self.state.remove_on_drop() {
            if let Err(error) = api::remove_dir_all(&self.path) {
                warn!(rundir = %self.path.display(), ?error, "failed to cleanup temporary runtime directory");
            }
        }
    }
}
struct State {
    inner: UnsafeCell<StateInner>,
}
#[derive(Copy, Clone, Debug)]
enum StateInner {
    Persistent,
    Temporary { remove_on_drop: bool },
}
impl State {
    #[allow(non_upper_case_globals)]
    const Persistent: Self = Self {
        inner: UnsafeCell::new(StateInner::Persistent),
    };
    #[allow(non_upper_case_globals)]
    const Temporary: Self = Self {
        inner: UnsafeCell::new(StateInner::Temporary {
            remove_on_drop: false,
        }),
    };
    fn inner(&self) -> &StateInner {
        unsafe { &*self.inner.get() }
    }
    fn inner_mut(&self) -> &mut StateInner {
        unsafe { &mut *self.inner.get() }
    }
    fn init<P: AsRef<Path>>(&self, path: P) -> io::Result<()> {
        if let StateInner::Temporary { remove_on_drop } = self.inner_mut() {
            api::create_dir_all(path)?;
            *remove_on_drop = true;
        }
        Ok(())
    }
    fn temporary(&self) -> bool {
        matches!(self.inner(), StateInner::Temporary { .. })
    }
    fn remove_on_drop(&self) -> bool {
        match unsafe { *self.inner.get() } {
            StateInner::Temporary { remove_on_drop } => remove_on_drop,
            _ => false,
        }
    }
}

trait ScopeExt {
    fn rundir_path(&self) -> Option<Cow<'static, Path>>;
    fn rundir_name<'a>(&self, name: &'a OsStr) -> Cow<'a, Path>;
}
impl ScopeExt for ScopeKey {
    fn rundir_path(&self) -> Option<Cow<'static, Path>> {
        match *self {
            Self::Global => api::run_dir().map(Cow::Borrowed),
            Self::Local => Some(Cow::Owned(match api::xdg_runtime_dir() {
                Some(path) if api::is_dir(&path) => path,
                _ => api::run_dir()?.join(format!("user/{}", api::euid())),
            })),
        }
    }
    fn rundir_name<'a>(&self, name: &'a OsStr) -> Cow<'a, Path> {
        match *self {
            Self::Global => Cow::Borrowed(Path::new(name)),
            Self::Local => {
                let uid = api::euid().to_string();
                let mut path = OsString::with_capacity(name.len() + 1 + uid.len());
                path.push(name);
                path.push("-");
                path.push(uid);
                Cow::Owned(PathBuf::from(path))
            }
        }
    }
}

pub enum ScopedRuntimeDirectory {
    Global(RuntimeDirectory<Global>),
    Local(RuntimeDirectory<Local>),
}
impl<T: DaemonScope> From<RuntimeDirectory<T>> for ScopedRuntimeDirectory {
    fn from(rundir: RuntimeDirectory<T>) -> Self {
        match T::key() {
            ScopeKey::Global => {
                ScopedRuntimeDirectory::Global(unsafe { std::mem::transmute(rundir) })
            }
            ScopeKey::Local => {
                ScopedRuntimeDirectory::Local(unsafe { std::mem::transmute(rundir) })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use claims::assert_ok;
    use daemonbit_core::{Global, Local};

    use super::*;

    const DAEMON_NAME: &str = "daemon";
    const USER: u32 = 1000;

    #[test]
    fn persistent_local_xdg_present() {
        api::mock(DAEMON_NAME, USER, Some("/run"), Some("/home/user/.run"));

        let rundir = assert_ok!(RuntimeDirectory::<Local>::get());
        assert_eq!(rundir.as_path(), Path::new("/home/user/.run/daemon"));
        assert!(!rundir.temporary());
    }

    #[test]
    fn persistent_local_xdg_missing() {
        api::mock(DAEMON_NAME, USER, Some("/run"), None);

        let rundir = assert_ok!(RuntimeDirectory::<Local>::get());
        assert_eq!(rundir.as_path(), Path::new("/run/user/1000/daemon"));
        assert!(!rundir.temporary());
    }

    #[test]
    fn temporary_local() {
        api::mock(DAEMON_NAME, USER, None, None);

        let rundir = assert_ok!(RuntimeDirectory::<Local>::get());
        assert_eq!(rundir.as_path(), Path::new("/tmp/daemon-1000"));
        assert!(rundir.temporary());
    }

    #[test]
    fn persistent_global_xdg_present() {
        api::mock(DAEMON_NAME, USER, Some("/run"), Some("/home/user/.run"));

        let rundir = assert_ok!(RuntimeDirectory::<Global>::get());
        assert_eq!(rundir.as_path(), Path::new("/run/daemon"));
        assert!(!rundir.temporary());
    }

    #[test]
    fn persistent_global_xdg_missing() {
        api::mock(DAEMON_NAME, USER, Some("/run"), None);

        let rundir = assert_ok!(RuntimeDirectory::<Global>::get());
        assert_eq!(rundir.as_path(), Path::new("/run/daemon"));
        assert!(!rundir.temporary());
    }

    #[test]
    fn temporary_global() {
        api::mock(DAEMON_NAME, USER, None, None);

        let rundir = assert_ok!(RuntimeDirectory::<Global>::get());
        assert_eq!(rundir.as_path(), Path::new("/tmp/daemon"));
        assert!(rundir.temporary());
    }
}
