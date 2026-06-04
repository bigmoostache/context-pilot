//! Lightweight utility replacements for external dependencies.
//!
//! This crate provides simple, focused implementations that replace heavier
//! external crates where only a small fraction of their functionality is used:
//!
//! - [`hash`] — Deterministic content hashing (replaces `sha2`)
//! - [`time`] — Timestamp formatting and parsing (replaces `chrono`)
//! - [`dirs`] — Platform-specific directories (replaces `dirs`)
//! - [`secret`] — Secret string wrapper (replaces `secrecy`)

pub mod dirs;
pub mod hash;
pub mod secret;
pub mod time;
