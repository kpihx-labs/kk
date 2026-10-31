// Copyright (c) Mike Grier.

//! Acceptance for the round-one catalogue, in two parts.
//!
//! The audit that produced the entry list had **two** purposes, and checking
//! only the first is how the coverage question got missed once already. So this
//! target is deliberately split:
//!
//! - [`operations`] re-expresses each audited call site against the catalogue
//!   and confirms every parameter shape the three consumers use is reachable.
//!   That is the test that the entry list came from real consumers rather than
//!   from taste.
//! - [`scenarios`] confirms the catalogue serves each consumer's actual
//!   **shape** -- not just its call list -- which is a different question and
//!   the one that was skipped.

mod support;

mod operations;
mod scenarios;
