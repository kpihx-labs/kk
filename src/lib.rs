//! Authenticated, explicit-address local messaging. See CONTRACT.md for guarantees.

pub mod config;
pub mod discovery;
pub mod envelope;
mod error;
pub mod files;
mod identity;
pub mod input;
pub mod profile;
mod session;
mod transport;

pub use error::{Error, Result};
pub use identity::{Identity, Peer};
pub use session::{Listener, ReceivedText, send, send_with_policy};

/// The package version, shared by the library and CLI.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
