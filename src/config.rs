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
pub const ROOM_FILE_SHARES: usize = 64;
pub const ROOM_FILE_SHARE_PATH_BYTES: usize = 4096;
pub const ROOM_MAX_WIRE_BYTES: usize = 1_048_576;
pub const ROOM_BOOTSTRAP_AUTHORITY_BYTES: usize = 512 * 1024;
pub const ROOM_MAX_MEMBERS: usize = 8;
pub const ROOM_TRANSPORT_PEERS: usize = 2 * ROOM_MAX_MEMBERS + 1;
pub const QUIC_SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(1);
pub const ROOM_KEY_PACKAGE_BUDGET: usize = 4;
pub const ROOM_KEY_PACKAGE_LIFETIME: u64 = 3600;
pub const ROOM_CLOCK_MARGIN: u64 = 60;
pub const ROOM_PACKAGE_REPLAY_LIMIT: usize = 256;
pub const ROOM_STORE_MAX_BYTES: usize = 16 * 1024 * 1024;
pub const ROOM_STORE_VALUE_BYTES: usize = 4 * 1024 * 1024;
pub const ROOM_CONTROL_WIRE_BYTES: usize = 3 * ROOM_MAX_WIRE_BYTES + 4096;
pub const ROOM_CONTROL_SNAPSHOT_BYTES: usize = 4 * 1024 * 1024;
pub const ROOM_CONTROL_DATABASE_BYTES: usize = 64 * 1024 * 1024;
pub const ROOM_CONTROL_DATABASE_VALUE_BYTES: usize = 12 * 1024 * 1024;
pub const ROOM_CONTROL_LOG_BATCH: usize = 4096;
pub const ROOM_CONTROL_RPC_BYTES: usize = 12 * 1024 * 1024;
pub const ROOM_CONTROL_CONNECTIONS: usize = 24;
pub const ROOM_CONTROL_RPCS: usize = 16;
pub const ROOM_CONTROL_SNAPSHOT_CHUNK: usize = 65_536;
pub const ROOM_WELCOME_OUTBOX_COUNT: usize = 16;
pub const ROOM_WELCOME_OUTBOX_BYTES: usize = 1024 * 1024;
pub const ROOM_WELCOME_REPLICA_BYTES: usize = 1024 * 1024;
pub const ROOM_CONTROL_RECEIPTS: usize = 2 * ROOM_PACKAGE_REPLAY_LIMIT;
pub const ROOM_APPLIED_RECORDS: usize = 64;
pub const ROOM_APPLIED_RECORD_BYTES: usize = 8 * 1024 * 1024;
pub const ROOM_COMMAND_QUEUE: usize = 64;
pub const ROOM_EVENT_QUEUE: usize = 64;
pub const ROOM_DATA_RPCS: usize = 4;
pub const MEDIA_PACKET_BYTES: usize = 1024;
pub const MEDIA_ENCODED_FRAME_BYTES: usize = 256 * 1024;
pub const MEDIA_DIAGNOSTIC_BYTES: usize = 8192;
pub const MEDIA_VIDEO_WIDTH: u16 = 640;
pub const MEDIA_VIDEO_HEIGHT: u16 = 360;
pub const MEDIA_VIDEO_FPS: u8 = 15;
pub const MEDIA_VIDEO_BITRATE: u32 = 800_000;
pub const MEDIA_AUDIO_BITRATE: u32 = 64_000;
pub const MEDIA_AUDIO_MAX_RATE: u64 = 128_000;
pub const MEDIA_VIDEO_MAX_RATE: u64 = 2_000_000;
pub const MEDIA_PROCESS_ADDRESS_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub const MEDIA_PROCESS_DATA_BYTES: u64 = 256 * 1024 * 1024;
pub const MEDIA_PACKET_QUEUE: usize = 8;
pub const MEDIA_MAX_STREAMS: usize = 4;
pub const MEDIA_DATAGRAM_BUFFER_BYTES: usize = 64 * (MEDIA_PACKET_BYTES + 16);
pub const MEDIA_POLICY_REFRESH: Duration = Duration::from_millis(500);
pub const MEDIA_POLICY_TIMEOUT: Duration = Duration::from_millis(1500);
pub const MEDIA_POLICY_LEASE: Duration = Duration::from_secs(2);
pub const MEDIA_PACKET_MAX_AGE: Duration = Duration::from_millis(250);
pub const MEDIA_SOURCE_IDLE: Duration = Duration::from_secs(5);
pub const MEDIA_MAX_DURATION: Duration = Duration::from_secs(3600);
pub const UI_MAX_ROOMS: usize = 64;
pub const UI_REQUEST_QUEUE: usize = 32;
pub const UI_HISTORY_LIMIT: usize = 256;
pub const UI_OPERATION_LIMIT: usize = 16;
pub const UI_MAX_TRANSFERS: usize = 4;
pub const UI_JSON_BYTES: usize = MAX_MESSAGE_BYTES * 6 + 8192;
pub const UI_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) const ALPN: &[u8] = b"kk-text/2";
pub(crate) const TLS_NAME: &str = "kk.local";
pub(crate) const WINDOW_BYTES: u32 = (MAX_MESSAGE_BYTES + MESSAGE_HEADER_BYTES) as u32;
