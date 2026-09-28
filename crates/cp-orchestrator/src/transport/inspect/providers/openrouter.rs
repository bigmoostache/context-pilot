//! The `OpenRouter` aggregator's curated model catalogue.
//!
//! Split from the parent registry so `providers/mod.rs` stays within the
//! 500-line structure cap. See [`provider_openrouter`] for the shape contract.

use super::{ModelDef, ProviderDef};

/// The `OpenRouter` aggregator and its curated model catalogue.
///
/// One flat provider list: `id` = the per-provider enum's serde name
/// (kebab-case, matched by `apply_configure`'s `from_value::<OpenRouterModel>`),
/// `api_name` = the full `vendor/slug[:tag]` sent verbatim in the request body.
/// The frontend groups these by the `vendor` prefix (sub-provider) — the backend
/// stays a flat list.
pub(super) fn provider_openrouter() -> ProviderDef {
    ProviderDef {
        id: "openrouter",
        name: "OpenRouter",
        description: "Aggregator \u{2014} Space Bunny \u{b7} GLM 5.3 \u{b7} Nemotron 3 \u{b7} DeepSeek V4.1",
        models: vec![
            ModelDef {
                id: "space-bunny-alpha",
                api_name: "stealth/space-bunny-alpha",
                display_name: "Space Bunny Alpha",
                context_window: 1_000_000,
                max_output: 128_000,
                input_price: 0.0,
                output_price: 0.0,
                badge: Some("Free"),
                is_default: true,
            },
            ModelDef {
                id: "glm53-flash",
                api_name: "z-ai/glm-5.3-flash",
                display_name: "GLM 5.3 Flash",
                context_window: 1_310_720,
                max_output: 128_000,
                input_price: 0.045,
                output_price: 0.14,
                badge: None,
                is_default: false,
            },
            ModelDef {
                id: "nemotron3-ultra",
                api_name: "nvidia/nemotron-3-ultra-550b-a55b:free",
                display_name: "Nemotron 3 Ultra (free)",
                context_window: 1_000_000,
                max_output: 128_000,
                input_price: 0.0,
                output_price: 0.0,
                badge: Some("Free"),
                is_default: false,
            },
            ModelDef {
                id: "deepseek-v41-flash",
                api_name: "deepseek/deepseek-v4.1-flash",
                display_name: "DeepSeek V4.1 Flash",
                context_window: 0x0010_0000,
                max_output: 128_000,
                input_price: 0.15,
                output_price: 0.6,
                badge: None,
                is_default: false,
            },
        ],
    }
}
