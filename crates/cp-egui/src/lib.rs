//! Context Pilot — egui desktop frontend.
//!
//! This crate provides a native GUI for Context Pilot using [`eframe`] and
//! [`egui`]. It consumes the platform-agnostic IR types from [`cp_render`]
//! and maps them to egui widgets, producing the same UI as the terminal
//! frontend but with mouse support, proportional fonts, and resizable panels.

/// Application struct and main update loop.
pub mod app;

/// Block → egui widget renderers (Line, Table, Tree, ProgressBar, etc.).
pub mod renderers;

/// Semantic → egui style mapping (palette, `RichText`, `LayoutJob`).
pub mod theme;
