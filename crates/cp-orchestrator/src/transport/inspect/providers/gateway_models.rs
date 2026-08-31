//! What the LLM gateway declares it can route — `GET /v1/models`, cached.
//!
//! The cockpit must not offer a model the proxy cannot resolve: routed through a
//! gateway, a model absent from its `model_list` fails with an opaque 400 on the
//! first message. `/v1/models` names exactly what that table declares, costs no
//! tokens, and needs only the key this process already holds — so the picker
//! intersects the catalogue with it.
//!
//! What it does **not** attest: whether the proxy holds a working provider key
//! behind a declared model. Only its `/health` knows that, and that endpoint
//! issues a real request per model — an operator action, not something to hang a
//! picker on.

use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

/// How long a successful snapshot is served before refetching. Long enough that
/// opening the picker repeatedly costs one call, short enough that adding a model
/// to `litellm.yaml` shows up without restarting the orchestrator.
const TTL_OK: Duration = Duration::from_mins(5);

/// How long a *failure* is remembered. Also a cache, deliberately: without it an
/// unreachable proxy would cost every picker load a full timeout.
const TTL_ERR: Duration = Duration::from_secs(30);

/// Ceiling on the request. The picker is a foreground call — a sick proxy must
/// degrade it, not hang it.
const TIMEOUT: Duration = Duration::from_secs(3);

/// One cached answer: when it was taken, and what it said. `None` inside means
/// "the gateway could not be asked", which callers must treat as "do not filter"
/// rather than "nothing is available".
struct Snapshot {
    /// When this answer was fetched.
    at: Instant,
    /// The declared model ids, or `None` if the fetch failed.
    models: Option<HashSet<String>>,
}

/// The process-wide cache. A mutex, not a `OnceLock`: the value expires.
static CACHE: LazyLock<Mutex<Option<Snapshot>>> = LazyLock::new(|| Mutex::new(None));

/// Model ids the gateway declares it can route.
///
/// `None` means "no filtering is possible" — no gateway is configured, or it
/// could not be reached. Callers keep their full catalogue in that case: a
/// transient proxy failure must not empty the cockpit's model picker, which would
/// read as the product losing its models.
fn declared() -> Option<HashSet<String>> {
    let base = cp_base::config::llm::gateway::base_url()?;
    let mut cache = CACHE.lock().ok()?;
    if let Some(snapshot) = cache.as_ref() {
        let ttl = if snapshot.models.is_some() { TTL_OK } else { TTL_ERR };
        if snapshot.at.elapsed() < ttl {
            return snapshot.models.clone();
        }
    }
    let fetched = fetch(&base);
    *cache = Some(Snapshot { at: Instant::now(), models: fetched.clone() });
    fetched
}

/// `GET <base>/v1/models`, returning the `data[].id` set. `None` on any transport
/// error, non-2xx status or unparseable body — every one of which means the same
/// thing here: we do not know, so we must not filter.
fn fetch(base: &str) -> Option<HashSet<String>> {
    let key = std::env::var(cp_base::config::llm::gateway::GATEWAY_KEY_ENV).unwrap_or_default();
    let response = reqwest::blocking::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .ok()?
        .get(format!("{base}/v1/models"))
        .header("Authorization", format!("Bearer {key}"))
        .send()
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let body: serde_json::Value = response.json().ok()?;
    let ids: HashSet<String> =
        body.get("data")?.as_array()?.iter().filter_map(|entry| entry.get("id")?.as_str().map(str::to_owned)).collect();
    // An empty list is a valid answer ("this proxy declares nothing"), but it is
    // indistinguishable from a shape we failed to understand — and the cost of
    // being wrong is an empty picker, so treat it as "unknown".
    if ids.is_empty() { None } else { Some(ids) }
}

// ── The picker's view ───────────────────────────────────────────────────

/// What the gateway will route, snapshotted once for one `GET /api/providers`
/// reply. Taken once rather than per model: the answer cannot change mid-reply,
/// and a per-model lookup would make an unreachable proxy cost one timeout per
/// catalogue entry.
pub(crate) struct Filter {
    /// The declared model ids, or `None` when the gateway could not be asked.
    declared: Option<HashSet<String>>,
}

impl Filter {
    /// Take the snapshot (served from the cache when it is warm).
    pub(crate) fn new() -> Self {
        Self { declared: declared() }
    }

    /// May `provider_id`'s `api_name` be offered to the cockpit?
    ///
    /// Yes for a provider the gateway does not carry, or when there is no
    /// gateway — the catalogue then decides alone. Yes as well when the proxy
    /// could not be asked: not knowing what it routes is a reason to keep the
    /// catalogue, not to empty the picker (a proxy that is briefly unreachable
    /// would otherwise look like a product that lost its models).
    pub(crate) fn offers(&self, provider_id: &str, api_name: &str) -> bool {
        let gated = cp_base::config::llm::gateway::is_active()
            && cp_base::config::llm::gateway::declared_in_model_list(provider_id);
        !gated || self.declared.as_ref().is_none_or(|set| set.contains(api_name))
    }
}

/// Does the gateway hold `provider_id`'s credential on this process's behalf?
///
/// Under a gateway the providers it carries are usable with **no local key at
/// all**: the credential that reaches the provider lives in the proxy, and this
/// process is not supposed to have a copy. Without this the cockpit offers
/// nothing on a gateway deployment — an empty model picker with no error
/// anywhere, which is not a diagnosable failure.
pub(crate) fn provides(provider_id: &str) -> bool {
    cp_base::config::llm::gateway::is_active() && cp_base::config::llm::gateway::routes_provider(provider_id)
}
