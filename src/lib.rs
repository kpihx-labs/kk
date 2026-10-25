//! Authenticated, explicit-address local messaging. See CONTRACT.md for guarantees.

pub mod config;
pub mod contacts;
pub mod discovery;
pub mod envelope;
mod error;
pub mod files;
mod identity;
pub mod input;
#[cfg(unix)]
pub mod node;
pub mod output;
pub mod pairing;
pub mod profile;
pub mod rooms;
mod session;
mod signed;
mod transport;

pub use error::{Error, Result};
pub use identity::{Identity, Peer};
pub use session::{Listener, ReceivedText, send, send_authorized, send_with_policy};

/// The package version, shared by the library and CLI.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
