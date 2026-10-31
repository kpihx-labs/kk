// Copyright (c) Mike Grier.

//! Integration tests for the handle-producing entries.
//!
//! These cross a real filesystem boundary and chain several entries together,
//! which is what separates them from the per-entry unit tests: those prove each
//! entry against Windows in isolation, and these prove the entries compose into
//! the shapes the audited consumers actually use.
//!
//! Laid out as `tests/<name>/main.rs` because a test crate root resolves
//! `mod x;` against the directory containing it. `tests/handle_entries.rs` plus
//! `tests/handle_entries/part.rs` would not compile as one target.

mod support;

mod composition;
mod flag_shapes;
