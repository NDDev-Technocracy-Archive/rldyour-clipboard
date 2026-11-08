//! The product's file configuration and local integration contract.
//!
//! Reading this contract does not open an archive, start capture or connect to a
//! daemon. Paths remain local data and are intentionally absent from Debug.

mod config;
pub mod content;
pub mod desktop;
pub mod files;
pub mod wire;

pub use config::{
    Config, ConfigError, FileConfig, config_schema, default_archive_dir, default_config_path,
    default_socket_path,
};

/// Existing local wire contract. A repository or product rename does not change it.
pub use wire::PROTOCOL_VERSION;

/// The maximum size of a control frame; content is streamed separately.
pub use wire::MAX_FRAME as MAX_FRAME_BYTES;

/// Existing local installation identity, preserved for enrolled clients.
pub const SOCKET_NAME: &str = "rldyour-clipboard.sock";
