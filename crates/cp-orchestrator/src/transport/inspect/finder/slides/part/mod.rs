//! OOXML package and XML primitives: the two lowest layers of the converter.
//!
//! Grouped in their own directory so `slides/` stays within the folder-entry
//! budget, and because both are package-format plumbing rather than slide
//! semantics: [`package`] unzips the pptx into addressable parts,
//! [`xml`] is the owned element tree those parts are parsed into.

pub(super) mod package;
pub(super) mod xml;
