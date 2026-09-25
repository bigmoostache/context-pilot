//! `OpenRouter` model enum — a curated shortlist of the aggregator's catalogue.
//!
//! Kept in its own file (the parent [`models`](super::models) re-exports it) so
//! that module stays under the structure line cap. `api_name` is the full
//! `vendor/slug[:tag]` id sent verbatim in the request body; the frontend groups
//! the picker by the `vendor` prefix (sub-provider). Metadata (context window,
//! pricing) was captured live from `OpenRouter`'s `/api/v1/models`.

use super::types::ModelInfo;

/// `OpenRouter` model variants — a curated shortlist of the aggregator's
/// catalogue.
///
/// `api_name` is the full `vendor/slug[:tag]` id; the frontend groups the picker
/// by the `vendor` prefix (sub-provider).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum OpenRouterModel {
    /// Space Bunny Alpha — anonymous stealth model, 1M context, free.
    #[default]
    #[serde(rename = "space-bunny-alpha")]
    SpaceBunnyAlpha,
    /// Z.ai GLM 5.3 Flash — efficient coding / long-horizon agent, ~1.3M context.
    #[serde(rename = "glm53-flash")]
    Glm53Flash,
    /// NVIDIA Nemotron 3 Ultra (free) — `MoE` frontier-reasoning model, 1M context.
    #[serde(rename = "nemotron3-ultra")]
    Nemotron3Ultra,
    /// DeepSeek V4.1 Flash — sparse `MoE`, ~1M context.
    #[serde(rename = "deepseek-v41-flash")]
    DeepSeekV41Flash,
}

impl ModelInfo for OpenRouterModel {
    fn api_name(&self) -> &'static str {
        match *self {
            Self::SpaceBunnyAlpha => "stealth/space-bunny-alpha",
            Self::Glm53Flash => "z-ai/glm-5.3-flash",
            Self::Nemotron3Ultra => "nvidia/nemotron-3-ultra-550b-a55b:free",
            Self::DeepSeekV41Flash => "deepseek/deepseek-v4.1-flash",
        }
    }

    fn display_name(&self) -> &'static str {
        match *self {
            Self::SpaceBunnyAlpha => "Space Bunny Alpha",
            Self::Glm53Flash => "GLM 5.3 Flash",
            Self::Nemotron3Ultra => "Nemotron 3 Ultra (free)",
            Self::DeepSeekV41Flash => "DeepSeek V4.1 Flash",
        }
    }

    fn context_window(&self) -> usize {
        match *self {
            Self::SpaceBunnyAlpha | Self::Nemotron3Ultra => 1_000_000,
            Self::Glm53Flash => 1_310_720,
            Self::DeepSeekV41Flash => 0x0010_0000,
        }
    }

    fn input_price_per_mtok(&self) -> f32 {
        match *self {
            Self::SpaceBunnyAlpha | Self::Nemotron3Ultra => 0.0,
            Self::Glm53Flash => 0.045,
            Self::DeepSeekV41Flash => 0.15,
        }
    }

    fn output_price_per_mtok(&self) -> f32 {
        match *self {
            Self::SpaceBunnyAlpha | Self::Nemotron3Ultra => 0.0,
            Self::Glm53Flash => 0.14,
            Self::DeepSeekV41Flash => 0.6,
        }
    }

    fn cache_hit_price_per_mtok(&self) -> f32 {
        self.input_price_per_mtok()
    }

    fn cache_miss_price_per_mtok(&self) -> f32 {
        self.input_price_per_mtok()
    }

    fn max_output_tokens(&self) -> u32 {
        128_000
    }
}
