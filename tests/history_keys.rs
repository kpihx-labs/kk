//! Standalone wiring until the SQL owner exports the store module.
pub use kk::{Error, Result, envelope};
#[allow(dead_code)]
#[path = "../src/rooms/store/history_key.rs"]
mod history_key;
