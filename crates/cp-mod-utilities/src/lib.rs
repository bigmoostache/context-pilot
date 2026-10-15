//! Lightweight utility replacements for external dependencies.
//!
//! This crate provides simple, focused implementations that replace heavier
//! external crates where only a small fraction of their functionality is used:
//!
//! - [`hash`] — Fast NON-cryptographic content hashing (FNV-1a; not a `sha2`
//!   substitute where collision resistance matters)
//! - [`time`] — Timestamp formatting and parsing, DST-aware local time
//!   (replaces `chrono`)
//! - [`dirs`] — Platform-specific directories (replaces `dirs`)
//! - [`secret`] — Secret string wrapper (replaces `secrecy`)
//! - [`cron`] — 5-field POSIX cron expressions (replaces `cron`)

mod calendar;
pub mod cron;
pub mod dirs;
pub mod hash;
pub mod secret;
pub mod time;
mod tz;
