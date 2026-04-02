//! Semantic → egui style mapping.
//!
//! Maps [`cp_render::Semantic`] colour tokens and [`cp_render::Span`]
//! modifiers to egui's [`RichText`](eframe::egui::RichText) and
//! [`Color32`](eframe::egui::Color32). The palette is defined once here
//! and consumed by every renderer in the crate.

use cp_render::{Semantic, Span};
use eframe::egui::{self, Color32, FontId, RichText, TextFormat};

// ── Dark palette ─────────────────────────────────────────────────────

/// Resolve a [`Semantic`] token to a concrete [`Color32`] (dark theme).
#[must_use]
pub const fn semantic_color(semantic: Semantic) -> Color32 {
    match semantic {
        Semantic::Accent | Semantic::Active | Semantic::Header => Color32::from_rgb(0, 215, 255),
        Semantic::AccentDim => Color32::from_rgb(0, 150, 180),
        Semantic::Muted => Color32::from_rgb(128, 128, 128),
        Semantic::Success | Semantic::DiffAdd => Color32::from_rgb(0, 255, 135),
        Semantic::Warning | Semantic::KeyHint => Color32::from_rgb(255, 215, 0),
        Semantic::Error | Semantic::DiffRemove => Color32::from_rgb(255, 85, 85),
        Semantic::Info => Color32::from_rgb(135, 175, 255),
        Semantic::Code => Color32::from_rgb(200, 200, 200),
        Semantic::Border => Color32::from_rgb(68, 68, 68),
        // Default, Bold, and any future non-exhaustive variants.
        Semantic::Default | Semantic::Bold | _ => Color32::from_rgb(220, 220, 220),
    }
}

// ── Span → RichText ──────────────────────────────────────────────────

/// Convert a [`Span`] into an egui [`RichText`] with full styling.
///
/// Handles semantic colour, RGB override (syntax highlighting), and
/// bold / italic / dimmed modifiers.
#[must_use]
pub fn span_to_rich_text(span: &Span) -> RichText {
    let color = span.color.map_or_else(|| semantic_color(span.semantic), |(r, g, b)| Color32::from_rgb(r, g, b));

    let font = if span.semantic == Semantic::Code { FontId::monospace(14.0) } else { FontId::proportional(14.0) };

    let mut rt = RichText::new(&span.text).color(color).font(font);

    if span.bold {
        rt = rt.strong();
    }
    if span.italic {
        rt = rt.italics();
    }
    if span.dimmed {
        rt = rt.weak();
    }

    rt
}

// ── Span slice → LayoutJob ───────────────────────────────────────────

/// Build a [`LayoutJob`](egui::text::LayoutJob) from multiple spans.
///
/// Use this when a single line contains mixed styling — egui needs a
/// `LayoutJob` to render heterogeneous text fragments inline.
#[must_use]
pub fn spans_to_layout_job(spans: &[Span]) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();

    for span in spans {
        let color = span.color.map_or_else(|| semantic_color(span.semantic), |(r, g, b)| Color32::from_rgb(r, g, b));

        let font_id =
            if span.semantic == Semantic::Code { FontId::monospace(14.0) } else { FontId::proportional(14.0) };

        let mut format = TextFormat { font_id, color, ..TextFormat::default() };

        if span.italic {
            format.italics = true;
        }

        job.append(&span.text, 0.0, format);
    }

    job
}

/// Font size multiplier for [`Semantic::Header`] spans.
pub const HEADER_FONT_SIZE: f32 = 18.0;

/// Standard body font size.
pub const BODY_FONT_SIZE: f32 = 14.0;
