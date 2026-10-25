//! What Luna knows about models before asking a provider.
//!
//! A missing entry is not an error: unknown models get a conservative
//! 128k context window and zero cost, which keeps custom endpoints usable.

use crate::types::{Cost, ModelInfo};

struct Known {
    provider: &'static str,
    id: &'static str,
    name: &'static str,
    reasoning: bool,
    context: u32,
    max_tokens: u32,
    input: &'static [&'static str],
    cost: Cost,
}

const fn price(input: f64, output: f64, cache_read: f64, cache_write: f64) -> Cost {
    Cost { input, output, cache_read, cache_write }
}

const KNOWN: &[Known] = &[
    Known {
        provider: "anthropic",
        id: "claude-sonnet-4-5",
        name: "Claude Sonnet 4.5",
        reasoning: true,
        context: 200_000,
        max_tokens: 64_000,
        input: &["text", "image"],
        cost: price(3.0, 15.0, 0.3, 3.75),
    },
    Known {
        provider: "anthropic",
        id: "claude-opus-4-1",
        name: "Claude Opus 4.1",
        reasoning: true,
        context: 200_000,
        max_tokens: 32_000,
        input: &["text", "image"],
        cost: price(15.0, 75.0, 1.5, 18.75),
    },
    Known {
        provider: "anthropic",
        id: "claude-haiku-4-5",
        name: "Claude Haiku 4.5",
        reasoning: true,
        context: 200_000,
        max_tokens: 32_000,
        input: &["text", "image"],
        cost: price(1.0, 5.0, 0.1, 1.25),
    },
    Known {
        provider: "openai",
        id: "gpt-5",
        name: "GPT-5",
        reasoning: true,
        context: 400_000,
        max_tokens: 128_000,
        input: &["text", "image"],
        cost: price(1.25, 10.0, 0.125, 0.0),
    },
    Known {
        provider: "openai",
        id: "gpt-5-mini",
        name: "GPT-5 mini",
        reasoning: true,
        context: 400_000,
        max_tokens: 128_000,
        input: &["text", "image"],
        cost: price(0.25, 2.0, 0.025, 0.0),
    },
    Known {
        provider: "openai",
        id: "gpt-4.1",
        name: "GPT-4.1",
        reasoning: false,
        context: 1_047_576,
        max_tokens: 32_768,
        input: &["text", "image"],
        cost: price(2.0, 8.0, 0.5, 0.0),
    },
    Known {
        provider: "gemini",
        id: "gemini-2.5-pro",
        name: "Gemini 2.5 Pro",
        reasoning: true,
        context: 1_048_576,
        max_tokens: 65_536,
        input: &["text", "image"],
        cost: price(1.25, 10.0, 0.31, 0.0),
    },
    Known {
        provider: "gemini",
        id: "gemini-2.5-flash",
        name: "Gemini 2.5 Flash",
        reasoning: true,
        context: 1_048_576,
        max_tokens: 65_536,
        input: &["text", "image"],
        cost: price(0.3, 2.5, 0.075, 0.0),
    },
];

/// Build the metadata for one model, using the catalogue when it has an entry.
pub fn describe(provider: &str, id: &str) -> ModelInfo {
    if let Some(known) = KNOWN.iter().find(|known| known.provider == provider && known.id == id) {
        return ModelInfo {
            provider: provider.to_string(),
            id: known.id.to_string(),
            name: known.name.to_string(),
            reasoning: known.reasoning,
            context_window: known.context,
            max_tokens: known.max_tokens,
            input: known.input.iter().map(|kind| kind.to_string()).collect(),
            cost: known.cost,
        };
    }
    ModelInfo {
        provider: provider.to_string(),
        id: id.to_string(),
        name: id.to_string(),
        reasoning: true,
        context_window: 128_000,
        max_tokens: 16_384,
        input: vec!["text".to_string(), "image".to_string()],
        cost: Cost::default(),
    }
}

/// Every catalogued model for a provider, used when discovery returns nothing.
pub fn for_provider(provider: &str) -> Vec<ModelInfo> {
    KNOWN
        .iter()
        .filter(|known| known.provider == provider)
        .map(|known| describe(provider, known.id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_models_keep_their_context_window() {
        let model = describe("anthropic", "claude-sonnet-4-5");
        assert_eq!(model.context_window, 200_000);
        assert!(model.reasoning);
    }

    #[test]
    fn unknown_models_get_a_safe_default() {
        let model = describe("local", "some-tiny-model");
        assert_eq!(model.context_window, 128_000);
        assert_eq!(model.cost.input, 0.0);
    }
}
