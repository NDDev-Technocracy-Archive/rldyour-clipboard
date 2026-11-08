//! Rust clipboard archive: thin CLI entry point, bounded sessions, native
//! capture, transactional index and leased content-addressed blobs.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
#![forbid(unsafe_code)]
mod capture;
mod cli;
mod config;
mod kind;
mod maintenance;
mod net;
mod outbox;
mod profile;
mod proto;
mod server;
mod session;
mod storage_fs;
mod store;
#[cfg(test)]
mod testing;
use std::process::ExitCode;

fn main() -> ExitCode {
    match cli::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("rldyour-clipboardd: {error}");
            ExitCode::FAILURE
        }
    }
}
