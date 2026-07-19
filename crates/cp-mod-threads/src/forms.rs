//! Send-time validation of agent-authored ` ```form ` blocks.
//!
//! Forms are a pure presentation layer: the agent writes a ` ```form ` fenced
//! block (YAML) inside a message's markdown, and the frontend renders it as an
//! interactive widget (see `docs/forms.md`). The backend stores the block as
//! opaque markdown — it never parses or tracks form state.
//!
//! The one exception is this module: a light **send-time guard** that catches a
//! malformed form before it reaches the thread (where it would render as
//! nothing). It is deliberately shallow — a real YAML parse would pull a new
//! dependency for zero durable benefit — so it only checks the two structural
//! invariants a usable form cannot omit: a `form-id` and a `fields` list. A
//! block missing either yields an error that fails the `Send` (design §7).

/// The fence that opens a form block. A line whose trimmed form equals this
/// starts a block; the next ```` ``` ```` line closes it.
const FENCE_OPEN: &str = "```form";
/// The generic closing fence.
const FENCE_CLOSE: &str = "```";

/// Validate every ` ```form ` block in `markdown`, returning one error string
/// per malformed block (empty when all blocks are well-formed or none exist).
///
/// A block is malformed when its body lacks a top-level `form-id:` key or a
/// `fields:` key. The `form-answer` block the frontend emits is **not** matched
/// here (its fence is `` ```form-answer ``, not `` ```form ``), so a user's
/// answer message is never validated as a form.
pub(crate) fn validate_form_blocks(markdown: &str) -> Vec<String> {
    let mut errors = Vec::new();
    for (idx, body) in form_block_bodies(markdown).into_iter().enumerate() {
        let has_id = body.lines().any(|l| l.trim_start().starts_with("form-id:"));
        let has_fields = body.lines().any(|l| l.trim_start().starts_with("fields:"));
        if !has_id || !has_fields {
            let n = idx.saturating_add(1);
            let mut missing = Vec::new();
            if !has_id {
                missing.push("form-id");
            }
            if !has_fields {
                missing.push("fields");
            }
            errors.push(format!("Malformed ```form``` block #{n}: missing `{}`", missing.join("`, `")));
        }
    }
    errors
}

/// Extract the body text (between the opening and closing fence) of every
/// ` ```form ` block. A `` ```form-answer `` fence is excluded — only a bare
/// `` ```form `` (optionally trailing whitespace) opens a block here.
fn form_block_bodies(markdown: &str) -> Vec<String> {
    let mut bodies = Vec::new();
    let mut lines = markdown.lines();
    while let Some(line) = lines.next() {
        if !is_form_open(line) {
            continue;
        }
        let mut body = String::new();
        for inner in lines.by_ref() {
            if inner.trim() == FENCE_CLOSE {
                break;
            }
            body.push_str(inner);
            body.push('\n');
        }
        bodies.push(body);
    }
    bodies
}

/// Whether `line` opens a form block: its trimmed form is exactly `` ```form ``
/// (rejecting `` ```form-answer `` and `` ```formatting `` etc.).
fn is_form_open(line: &str) -> bool {
    line.trim() == FENCE_OPEN
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn well_formed_block_passes() {
        let md = "intro\n```form\nform-id: x\nfields:\n  - id: a\n```\ntail";
        assert!(validate_form_blocks(md).is_empty());
    }

    #[test]
    fn missing_form_id_fails() {
        let md = "```form\nfields:\n  - id: a\n```";
        let errs = validate_form_blocks(md);
        assert_eq!(errs.len(), 1);
        assert!(errs.first().is_some_and(|e| e.contains("form-id")));
    }

    #[test]
    fn missing_fields_fails() {
        let md = "```form\nform-id: x\n```";
        let errs = validate_form_blocks(md);
        assert_eq!(errs.len(), 1);
        assert!(errs.first().is_some_and(|e| e.contains("fields")));
    }

    #[test]
    fn form_answer_block_is_not_validated() {
        // A form-answer block must NOT be treated as a form (different fence).
        let md = "```form-answer\nform-id: x\nanswers: []\n```";
        assert!(validate_form_blocks(md).is_empty());
    }

    #[test]
    fn no_blocks_is_clean() {
        assert!(validate_form_blocks("just prose, no forms").is_empty());
    }

    #[test]
    fn two_blocks_one_bad() {
        let md = "```form\nform-id: a\nfields: []\n```\n```form\nfields: []\n```";
        let errs = validate_form_blocks(md);
        assert_eq!(errs.len(), 1);
        assert!(errs.first().is_some_and(|e| e.contains("#2")));
    }
}
