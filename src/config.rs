//! Protocol constants and resource bounds for the one-message stage.

use std::time::Duration;

/// Maximum UTF-8 payload, excluding the four-byte length prefix.
pub const MAX_MESSAGE_BYTES: usize = 65_536;
pub const MESSAGE_HEADER_BYTES: usize = 5;
pub const MAX_AGE_HEADER_BYTES: usize = 65_536;
/// Bound before parsing any certificate or private-key file.
pub const MAX_IDENTITY_BYTES: usize = 16_384;
/// Default whole-operation deadline, including handshake and receipt.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
/// Upper limit for callers; shorter deadlines are useful for cancellation tests.
pub const MAX_TIMEOUT: Duration = Duration::from_secs(300);
/// Discovery is intentionally short-lived and results are always untrusted.
pub const MAX_DISCOVERY_TIME: Duration = Duration::from_secs(15);
pub const DEFAULT_DISCOVERY_TIME: Duration = Duration::from_secs(3);
pub const MAX_DISCOVERED_PEERS: usize = 64;
pub const FILE_CHUNK_BYTES: usize = 65_536;
pub const DEFAULT_FILE_LIMIT: u64 = 64 * 1024 * 1024;
pub const MAX_FILE_BYTES: u64 = 1024 * 1024 * 1024 * 1024;
pub const DEFAULT_FILE_TIMEOUT: Duration = Duration::from_secs(300);
pub const MAX_FILE_TIMEOUT: Duration = Duration::from_secs(86_400);

pub(crate) const ALPN: &[u8] = b"kk-text/2";
pub(crate) const TLS_NAME: &str = "kk.local";
pub(crate) const WINDOW_BYTES: u32 = (MAX_MESSAGE_BYTES + MESSAGE_HEADER_BYTES) as u32;
