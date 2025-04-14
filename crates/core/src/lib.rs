// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) $year Kamil Becmer

use std::fmt;

#[const_env::from_env]
const DAEMON_NAME: &'static str = "";

pub const fn daemon_name() -> &'static str {
    if DAEMON_NAME.len() == 0 {
        panic!("You have to set DAEMON_NAME env var")
    }
    DAEMON_NAME
}

pub trait DaemonScope: sealed::Sealed {
    fn key() -> ScopeKey;
}

pub enum Global {}
impl DaemonScope for Global {
    fn key() -> ScopeKey {
        ScopeKey::Global
    }
}

pub enum Local {}
impl DaemonScope for Local {
    fn key() -> ScopeKey {
        ScopeKey::Local
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum ScopeKey {
    Global,
    Local,
}
impl ScopeKey {
    pub fn from_str<S: AsRef<str>>(scope: S) -> Option<Self> {
        match scope.as_ref() {
            "global" => Some(Self::Global),
            "local" => Some(Self::Local),
            _ => None,
        }
    }
    pub const fn as_str(&self) -> &str {
        match *self {
            Self::Global => "global",
            Self::Local => "local",
        }
    }
}
impl fmt::Display for ScopeKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

pub trait Scoped {
    fn scope(&self) -> ScopeKey;
}
pub trait Acquire<T: DaemonScope>: Sized {
    fn acquire() -> Self;
}
pub trait TryAcquire<T: DaemonScope>: Sized {
    type Error: std::error::Error;
    fn try_acquire() -> Result<Self, Self::Error>;
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::Global {}
    impl Sealed for super::Local {}
}
