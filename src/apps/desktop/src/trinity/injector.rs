//! Trinity implementation of the core cognitive injector.
//!
//! Two channels, mirroring the cognitive engine's own contract:
//!
//! - `cognitive_protocol` returns the session-stable protocol (identity + NAP
//!   + cognitive instructions). The execution engine prepends it to the system
//!   prompt; the daemon returns the same bytes every turn so the prompt prefix
//!   stays cacheable.
//! - `cognitive_state_for` returns the live PSI state for the current user
//!   message. The execution engine prepends it to the latest user message, not
//!   the system prompt, so per-turn state never invalidates the cached prefix.
//!
//! Both degrade gracefully: when the daemon is unreachable they return `None`
//! and the execution engine proceeds unchanged.

use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use openbitfun_core::agentic::execution::cognitive_injector::CognitiveInjector;

use super::backend;

/// Turn-level state cache TTL: tool rounds inside one user turn are usually
/// closer together than this, so the same turn reuses one state snapshot.
const TURN_TTL: Duration = Duration::from_secs(5);

pub(crate) struct TrinityInjector {
    /// Turn-level cache: ((turn_key, user_message) fingerprint, state, at).
    /// A new turn or a changed message invalidates it immediately.
    turn_cache: Mutex<Option<((String, String), String, Instant)>>,
}

impl TrinityInjector {
    pub(crate) fn new() -> Self {
        Self {
            turn_cache: Mutex::new(None),
        }
    }
}

#[async_trait]
impl CognitiveInjector for TrinityInjector {
    /// Session-stable cognitive protocol (identity + NAP + cognitive tools).
    async fn cognitive_protocol(&self) -> Option<String> {
        backend::static_prompt().await
    }

    /// Live cognitive state without user-message context.
    async fn cognitive_state(&self) -> Option<String> {
        self.cognitive_state_for("", "").await
    }

    /// Live cognitive state for the current user message and turn.
    async fn cognitive_state_for(&self, user_message: &str, turn_key: &str) -> Option<String> {
        if let Ok(guard) = self.turn_cache.lock() {
            if let Some(((cached_turn, cached_msg), text, at)) = guard.as_ref() {
                if at.elapsed() < TURN_TTL && cached_turn == turn_key && cached_msg == user_message
                {
                    return Some(text.clone());
                }
            }
        }

        let state = backend::cognitive_state_block(user_message, turn_key).await?;
        if let Ok(mut guard) = self.turn_cache.lock() {
            *guard = Some((
                (turn_key.to_string(), user_message.to_string()),
                state.clone(),
                Instant::now(),
            ));
        }
        Some(state)
    }
}

/// Register the cognitive injector (idempotent).
pub(crate) fn register_trinity_injector() {
    let injector = Arc::new(TrinityInjector::new());
    if openbitfun_core::agentic::execution::cognitive_injector::set_cognitive_injector(injector) {
        log::info!("[trinity] cognitive injector registered");
    } else {
        log::warn!("[trinity] cognitive injector already registered, skipped");
    }
}
