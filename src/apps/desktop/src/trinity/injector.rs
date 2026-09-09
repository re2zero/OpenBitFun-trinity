//! Trinity implementations of the generic cognitive hooks.
//!
//! - `TrinityInjector` decorates the per-turn system prompt with the static
//!   identity + NAP protocol block and the live PSI cognitive state.
//! - `TrinitySamplingParams` supplies the per-turn sampling temperature from
//!   the PSI engine.
//!
//! Both degrade gracefully: when the daemon is unreachable they return `None`
//! and the execution engine proceeds unchanged.

use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use openbitfun_core::agentic::execution::cognitive_hooks::{
    SamplingParamsProvider, TurnPromptDecorator,
};

use super::backend;

/// Static prompt cache TTL: the identity + NAP block is stable across turns.
const STATIC_PROMPT_TTL: Duration = Duration::from_secs(60);
/// Per-turn cognitive state TTL: one user turn usually spans several rounds.
const COGNITIVE_STATE_TTL: Duration = Duration::from_secs(5);

pub(crate) struct TrinityInjector {
    static_cache: Mutex<Option<(String, Instant)>>,
    state_cache: Mutex<Option<(String, Instant)>>,
}

impl Default for TrinityInjector {
    fn default() -> Self {
        Self::new()
    }
}

impl TrinityInjector {
    pub(crate) fn new() -> Self {
        Self {
            static_cache: Mutex::new(None),
            state_cache: Mutex::new(None),
        }
    }

    async fn static_prompt_cached(&self) -> Option<String> {
        if let Ok(guard) = self.static_cache.lock() {
            if let Some((prompt, at)) = guard.as_ref() {
                if at.elapsed() < STATIC_PROMPT_TTL {
                    return Some(prompt.clone());
                }
            }
        }
        let prompt = backend::static_prompt().await?;
        if let Ok(mut guard) = self.static_cache.lock() {
            *guard = Some((prompt.clone(), Instant::now()));
        }
        Some(prompt)
    }

    async fn cognitive_state_cached(&self) -> Option<String> {
        if let Ok(guard) = self.state_cache.lock() {
            if let Some((state, at)) = guard.as_ref() {
                if at.elapsed() < COGNITIVE_STATE_TTL {
                    return Some(state.clone());
                }
            }
        }
        let state = backend::cognitive_state_block().await?;
        if let Ok(mut guard) = self.state_cache.lock() {
            *guard = Some((state.clone(), Instant::now()));
        }
        Some(state)
    }
}

#[async_trait]
impl TurnPromptDecorator for TrinityInjector {
    async fn decorate_system_prompt(&self) -> Option<String> {
        let mut parts = Vec::new();
        if let Some(identity) = self.static_prompt_cached().await {
            parts.push(identity);
        }
        if let Some(state) = self.cognitive_state_cached().await {
            parts.push(state);
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join("\n\n"))
        }
    }
}

pub(crate) struct TrinitySamplingParams;

#[async_trait]
impl SamplingParamsProvider for TrinitySamplingParams {
    async fn sampling_temperature(&self) -> Option<f64> {
        backend::sampling_temperature().await
    }
}

/// Register both hooks into the execution engine (idempotent).
pub(crate) fn register_cognitive_hooks() {
    let injector = Arc::new(TrinityInjector::new());
    if openbitfun_core::agentic::execution::cognitive_hooks::register_turn_prompt_decorator(
        injector,
    ) {
        log::info!("[trinity] turn prompt decorator registered");
    } else {
        log::warn!("[trinity] turn prompt decorator already registered, skipped");
    }
    if openbitfun_core::agentic::execution::cognitive_hooks::register_sampling_params_provider(
        Arc::new(TrinitySamplingParams),
    ) {
        log::info!("[trinity] sampling params provider registered");
    } else {
        log::warn!("[trinity] sampling params provider already registered, skipped");
    }
}