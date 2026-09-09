//! Optional turn-level cognitive hooks (generic, host-agnostic).
//!
//! These are deliberately generic extension points so an external cognitive
//! engine (e.g. Trinity) can decorate the per-turn system prompt and override
//! sampling parameters without forking the execution engine.
//!
//! When no provider is registered every hook is a no-op, so the execution
//! engine behaves exactly as before. Registration is first-wins and idempotent.

use std::sync::Arc;
use std::sync::OnceLock;

use async_trait::async_trait;

/// Decorates the per-turn system prompt with additional content.
#[async_trait]
pub trait TurnPromptDecorator: Send + Sync {
    /// Extra text to prepend to the system prompt, or `None` to leave it unchanged.
    async fn decorate_system_prompt(&self) -> Option<String>;
}

/// Supplies per-turn sampling parameters (e.g. temperature) to the LLM request.
#[async_trait]
pub trait SamplingParamsProvider: Send + Sync {
    /// Temperature override for this turn, or `None` to keep the resolved value.
    async fn sampling_temperature(&self) -> Option<f64>;
}

static TURN_PROMPT_DECORATOR: OnceLock<Arc<dyn TurnPromptDecorator>> = OnceLock::new();
static SAMPLING_PARAMS_PROVIDER: OnceLock<Arc<dyn SamplingParamsProvider>> = OnceLock::new();

/// Register the turn prompt decorator (first registration wins; idempotent).
pub fn register_turn_prompt_decorator(decorator: Arc<dyn TurnPromptDecorator>) -> bool {
    TURN_PROMPT_DECORATOR.set(decorator).is_ok()
}

/// Register the sampling params provider (first registration wins; idempotent).
pub fn register_sampling_params_provider(provider: Arc<dyn SamplingParamsProvider>) -> bool {
    SAMPLING_PARAMS_PROVIDER.set(provider).is_ok()
}

/// Apply the registered decorator to a resolved system prompt (no-op when unset).
pub(crate) async fn decorate_system_prompt(base: String) -> String {
    let Some(decorator) = TURN_PROMPT_DECORATOR.get() else {
        return base;
    };
    match decorator.decorate_system_prompt().await {
        Some(extra) if !extra.trim().is_empty() => format!("{extra}\n\n{base}"),
        _ => base,
    }
}

/// Apply the registered sampling params provider to an AI client (no-op when unset).
pub(crate) async fn apply_sampling_params_override(
    client: Arc<crate::infrastructure::ai::AIClient>,
) -> Arc<crate::infrastructure::ai::AIClient> {
    let Some(provider) = SAMPLING_PARAMS_PROVIDER.get() else {
        return client;
    };
    let Some(temperature) = provider.sampling_temperature().await else {
        return client;
    };
    if client.config.temperature == Some(temperature) {
        return client;
    }
    let mut derived = client.as_ref().clone();
    derived.config.temperature = Some(temperature);
    Arc::new(derived)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hooks must be exact no-ops while unregistered so the execution
    /// engine behaves identically to upstream.
    #[tokio::test]
    async fn hooks_are_noop_when_unregistered() {
        if TURN_PROMPT_DECORATOR.get().is_some() || SAMPLING_PARAMS_PROVIDER.get().is_some() {
            // Another test registered a provider; skip rather than assert on
            // shared global state.
            return;
        }
        let base = "base system prompt".to_string();
        assert_eq!(decorate_system_prompt(base.clone()).await, base);
    }
}