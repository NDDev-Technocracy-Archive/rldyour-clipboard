//! Safe Rust boundary for the direct OS operations permitted by ADR 0001.
//! This crate owns no archive, policy, database, collector or NDS integration.

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::{ActivatedListener, inherited_listener};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::is_unix_socket;
