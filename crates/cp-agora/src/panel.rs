use crossterm::event::KeyEvent;

use cp_base::panels::{CacheRequest, CacheUpdate, ContextItem, Panel};
use cp_base::state::actions::Action;
use cp_base::state::context::Entry;
use cp_base::state::context::{Kind, estimate_tokens};
use cp_base::state::runtime::State;

use cp_base::panels::scroll_key_action;
use std::fmt::Write as _;

use crate::types::AgoraState;

/// Render the identity and the fleet as a YAML-style string for the LLM
/// context block.
///
/// The agent's own identity comes first, then its peers. The profile (slug and
/// image) is deliberately absent for *this* agent — it is human-facing render
/// data — but a peer's slug IS included, because a name is the only handle the
/// agent has for referring to a neighbour.
fn format_identity_for_context(state: &State) -> String {
    let ag = AgoraState::get(state);
    let mut out = String::new();

    if ag.identity.is_empty() {
        out.push_str("identity: (not set - use Agora_set_identity to define it)\n");
    } else {
        for (key, val) in ag.identity.pairs() {
            let _r = writeln!(out, "{key}: {val}");
        }
    }

    // An absent section reads as "no neighbours", which is honest and shorter
    // than an empty list. The fleet is the *other* agents, so on a single-agent
    // box there is genuinely nothing to say.
    if !ag.fleet.is_empty() {
        out.push_str("\nfleet:\n");
        for peer in &ag.fleet {
            let _wrote_slug = writeln!(out, "  - slug: {}", peer.slug);
            let _wrote_path = writeln!(out, "    path: {}", peer.path);
            if peer.identity.is_empty() {
                out.push_str("    identity: (not set)\n");
            } else {
                out.push_str("    identity:\n");
                for (key, val) in peer.identity.pairs() {
                    let _wrote_key = writeln!(out, "      {key}: {val}");
                }
            }
        }
    }

    out.trim_end().to_owned()
}

/// One YAML-style `key: value` row, indented to `indent` spaces.
///
/// Shared by the self-identity rows and the peer rows so both sides of the
/// panel line up under one renderer rather than drifting apart.
fn row(indent: usize, key: &str, val: &str) -> cp_render::Block {
    use cp_render::{Block, Semantic, Span as S};
    Block::KeyValue(vec![(
        vec![S::muted(format!("{:indent$}{key}: ", "", indent = indent))],
        vec![S::styled(val.to_owned(), Semantic::Code)],
    )])
}

/// The fleet section: one indented YAML list item per peer.
///
/// Split out of [`Panel::blocks`] to keep that method within its complexity
/// budget, and because "how a neighbour is drawn" is a self-contained question
/// from "what this panel contains".
fn fleet_blocks(fleet: &[crate::types::PeerAgent]) -> Vec<cp_render::Block> {
    use cp_render::{Block, Span as S};

    if fleet.is_empty() {
        return Vec::new();
    }
    let mut blocks = vec![Block::Line(vec![S::muted("  fleet:".into())])];
    for peer in fleet {
        blocks.push(row(4, "- slug", &peer.slug));
        blocks.push(row(6, "path", &peer.path));
        if peer.identity.is_empty() {
            blocks.push(Block::Line(vec![S::muted("      identity: (not set)".into()).italic()]));
            continue;
        }
        blocks.push(Block::Line(vec![S::muted("      identity:".into())]));
        blocks.extend(peer.identity.pairs().into_iter().map(|(key, val)| row(8, key, val)));
    }
    blocks
}

/// Fixed panel rendering the agent's self-identity as a YAML block.
pub(crate) struct AgoraPanel;

impl Panel for AgoraPanel {
    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        scroll_key_action(key)
    }

    fn blocks(&self, state: &State) -> Vec<cp_render::Block> {
        use cp_render::{Block, Span as S};

        let ag = AgoraState::get(state);

        // Profile first (slug + image), then the identity prose. Each profile
        // field is skipped when unset so a fresh agent shows no empty rows.
        let mut blocks: Vec<Block> = Vec::new();
        if !ag.slug.is_empty() {
            blocks.push(row(2, "slug", &ag.slug));
        }
        if !ag.image.is_empty() {
            blocks.push(row(2, "image", &ag.image));
        }

        if ag.identity.is_empty() {
            if blocks.is_empty() {
                blocks
                    .push(Block::Line(vec![S::muted("  Identity not set - use Agora_set_identity.".into()).italic()]));
            }
        } else {
            blocks.extend(ag.identity.pairs().into_iter().map(|(key, val)| row(2, key, val)));
        }

        // Rendered even when this agent has said nothing about itself: who the
        // neighbours are does not depend on whether we introduced ourselves.
        blocks.extend(fleet_blocks(&ag.fleet));
        blocks
    }

    fn title(&self, _state: &State) -> String {
        "Agora".to_owned()
    }

    /// Rebuild the context block. The fleet itself is **not** read here — it is
    /// polled from the main loop's background phase
    /// ([`fleet::poll`](crate::fleet::poll)), so this method has one job and the
    /// peer cache has one owner. Two drivers mutating it would race with nobody
    /// responsible for the result.
    fn refresh(&self, state: &mut State) {
        let content = format_identity_for_context(state);
        let token_count = estimate_tokens(&content);

        for ctx in &mut state.context {
            if ctx.context_type.as_str() == Kind::AGORA {
                ctx.token_count = token_count;
                let _changed = cp_base::panels::update_if_changed(ctx, &content);
                break;
            }
        }
    }

    fn max_freezes(&self) -> u8 {
        // Identity mutates rarely (only on set_identity), so its bytes are
        // stable turn-to-turn and never break the cache on their own. The
        // freeze budget only matters WHEN an edit lands mid-cache-window: it
        // lets the (now-changed) render defer up to 4 times so a single
        // identity edit doesn't instantly evict the surrounding cache block.
        4
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        let content = format_identity_for_context(state);
        let (id, last_refresh_ms) = state
            .context
            .iter()
            .find(|c| c.context_type.as_str() == Kind::AGORA)
            .map_or(("P10", 0), |c| (c.id.as_str(), c.last_refresh_ms));
        vec![ContextItem::new(id, "Agora", content, last_refresh_ms)]
    }

    fn needs_cache(&self) -> bool {
        false
    }

    fn refresh_cache(&self, _request: CacheRequest) -> Option<CacheUpdate> {
        None
    }

    fn build_cache_request(&self, _ctx: &Entry, _state: &State) -> Option<CacheRequest> {
        None
    }

    fn apply_cache_update(&self, _update: CacheUpdate, _ctx: &mut Entry, _state: &mut State) -> bool {
        false
    }

    fn cache_refresh_interval_ms(&self) -> Option<u64> {
        None
    }

    fn suicide(&self, _ctx: &Entry, _state: &State) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PeerAgent;

    /// A peer with no identity must still render its slug and path: an agent
    /// that never introduced itself is a normal state, not a reason to hide it.
    #[test]
    fn a_silent_peer_still_shows_slug_and_path() {
        let blocks = fleet_blocks(&[PeerAgent {
            slug: "alpaca".to_owned(),
            path: "/Users/someone/code/alpaca".to_owned(),
            identity: crate::types::SelfIdentity::default(),
        }]);
        // header + slug + path + the "(not set)" line, and nothing more: an
        // empty identity must NOT expand into ten blank rows.
        assert_eq!(blocks.len(), 4);
    }

    /// An identified peer expands into its ten identity rows, so the count is
    /// the header plus slug, path, the `identity:` label and the ten keys.
    #[test]
    fn an_identified_peer_expands_to_ten_rows() {
        let mut identity = crate::types::SelfIdentity::default();
        identity.role = "builder".to_owned();
        let blocks = fleet_blocks(&[PeerAgent {
            slug: "tessera".to_owned(),
            path: "/Users/someone/code/tessera".to_owned(),
            identity,
        }]);
        assert_eq!(blocks.len(), 14);
    }

    /// No peers means no section at all — an empty `fleet:` header would claim
    /// the agent looked and found nothing to say about neighbours it does have.
    #[test]
    fn no_peers_renders_no_section() {
        assert!(fleet_blocks(&[]).is_empty());
    }
}
