//! Workspace-wide declarative macros.
//!
//! Dependency-free so that every crate — including the foundational ones
//! (`cp-wire`, `cp-vault`, `cp-oplog`) — can share a single audited lint
//! chokepoint instead of carrying its own suppression.

/// Match a shared reference by dereferencing the place, funneling the one
/// `ref`-binding suppression the workspace needs into a single audited site.
///
/// `clippy::pattern_type_mismatch` (forbid) rejects matching a variant pattern
/// against a `&Enum` via match ergonomics; its mandated fix is to dereference
/// the scrutinee (`match *place`) and bind non-`Copy` fields with `ref`. But
/// `clippy::ref_patterns` (also forbid) rejects `ref`. The two restriction
/// lints are mutually exclusive for destructuring a non-`Copy` field out of a
/// shared reference, so every such site routes through this macro — the single
/// ref-pattern suppression inside it covers all expansions.
///
/// Enums whose matched fields are all `Copy` (or fieldless) need no `ref` and
/// should use a plain `match *place` instead — this macro is only for the
/// irreducible non-`Copy` case. A `macro_rules!` cannot inspect its arms, so
/// that rule is enforced by review, not by the compiler.
///
/// ```
/// enum Shape {
///     Named(String),
///     Count(u32),
/// }
///
/// fn describe(shape: &Shape) -> String {
///     cp_macros::deref_match!(shape, {
///         Shape::Named(ref s) => s.clone(),
///         Shape::Count(n) => n.to_string(),
///     })
/// }
///
/// assert_eq!(describe(&Shape::Named("a".to_owned())), "a");
/// assert_eq!(describe(&Shape::Count(3)), "3");
/// ```
#[macro_export]
macro_rules! deref_match {
    ($place:expr, { $($arm:tt)* }) => {{
        #[expect(
            clippy::ref_patterns,
            reason = "clippy::pattern_type_mismatch mandates ref bindings when destructuring non-Copy fields out of a shared reference; the two restriction lints are mutually exclusive, so the deref-plus-ref form is funneled through this one macro"
        )]
        match *$place { $($arm)* }
    }};
}

#[cfg(test)]
mod tests;
