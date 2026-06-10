//! Panels + SQL result formatting.
//!
//! Groups the two entity panels (fixed schema panel + dynamic result panel)
//! with the shared formatting helpers they render through.

/// SQL result formatting utilities (shared by tools and panels).
pub(crate) mod format;
/// Fixed Entities panel — live schema, sample data, and empty-state guide.
pub(crate) mod panel;
/// Dynamic entity result panel — large query results, static + live refresh.
pub(crate) mod result_panel;
