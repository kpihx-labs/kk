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
pub const NODE_MAX_PEERS: usize = 8;
pub const NODE_MAX_CONNECTIONS: usize = NODE_MAX_PEERS * 3;
pub const NODE_MAX_STREAMS: usize = 16;
pub const NODE_EVENT_QUEUE: usize = 64;
pub const NODE_MAX_FILES: usize = 2;
pub const NODE_FILE_BUDGET: u64 = 256 * 1024 * 1024;
pub const NODE_FILE_COUNT_BUDGET: u64 = 128;
pub const ROOM_MAX_WIRE_BYTES: usize = 1_048_576;
pub const ROOM_MAX_MEMBERS: usize = 8;
pub const ROOM_KEY_PACKAGE_BUDGET: usize = 4;
pub const ROOM_KEY_PACKAGE_LIFETIME: u64 = 3600;
pub const ROOM_CLOCK_MARGIN: u64 = 60;
pub const ROOM_PACKAGE_REPLAY_LIMIT: usize = 256;
pub const ROOM_STORE_MAX_BYTES: usize = 16 * 1024 * 1024;
pub const ROOM_STORE_VALUE_BYTES: usize = 4 * 1024 * 1024;

pub(crate) const ALPN: &[u8] = b"kk-text/2";
pub(crate) const TLS_NAME: &str = "kk.local";
pub(crate) const WINDOW_BYTES: u32 = (MAX_MESSAGE_BYTES + MESSAGE_HEADER_BYTES) as u32;
