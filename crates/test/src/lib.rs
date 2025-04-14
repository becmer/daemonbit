use std::{
    io,
    ops::{Deref, DerefMut},
    sync::Arc,
};

use memchr::memchr;
use parking_lot::Mutex;
use tracing::Dispatch;
use tracing_subscriber::FmtSubscriber;

pub fn capture_tracing<F, T>(f: F) -> CapturedTracing<T>
where
    F: FnOnce() -> T,
{
    let buf = InnerBuffer::new();
    let dispatch: Dispatch = FmtSubscriber::builder()
        // .with_env_filter("trace")
        .with_writer({
            let buf = buf.clone();
            move || buf.clone()
        })
        .with_level(true)
        .with_ansi(false)
        .into();
    let result = tracing::dispatcher::with_default(&dispatch, f);
    CapturedTracing {
        result,
        buf: buf.into_inner(),
    }
}

pub struct CapturedTracing<T> {
    result: T,
    buf: Vec<u8>,
}
#[allow(unused)]
impl<T> CapturedTracing<T> {
    pub fn into_result(self) -> T {
        self.result
    }
    pub fn logged<S: AsRef<str>>(&self, s: S) -> bool {
        let s = s.as_ref().as_bytes();
        let (needle, rest) = match *s {
            [needle, ref rest @ ..] => (needle, rest),
            _ => return false,
        };

        let mut haystack = &*self.buf;
        while let Some(p) = memchr(needle, haystack) {
            haystack = &haystack[p + 1..];
            if rest.len() > haystack.len() {
                break;
            }
            if &haystack[..rest.len()] == rest {
                return true;
            }
        }
        false
    }
}
impl<T> Deref for CapturedTracing<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.result
    }
}
impl<T> DerefMut for CapturedTracing<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.result
    }
}

#[derive(Clone)]
struct InnerBuffer {
    shared: Arc<Mutex<Vec<u8>>>,
}
impl InnerBuffer {
    fn new() -> Self {
        Self {
            shared: Arc::new(Mutex::new(Vec::new())),
        }
    }
    fn into_inner(self) -> Vec<u8> {
        let mut shared = self.shared.lock();
        std::mem::take(&mut *shared)
    }
}
impl io::Write for InnerBuffer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.shared.lock().write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
