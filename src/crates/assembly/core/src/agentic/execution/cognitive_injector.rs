//! Cognitive injector: two-channel injection owned by an external cognitive
//! engine (e.g. Trinity).
//!
//! Channel 1 (`cognitive_protocol`) prepends the session-stable cognitive
//! protocol to the system prompt. Its content must stay byte-identical across
//! turns so the provider prompt/KV prefix cache keeps hitting.
//!
//! Channel 2 (`cognitive_state_for`) prepends the per-turn cognitive state to
//! the latest user message. It is deliberately NOT part of the system prompt:
//! live state changes every turn, and putting it there would invalidate the
//! cached prefix on every request.
//!
//! When no injector is registered every channel is a no-op, so the execution
//! engine behaves exactly as before. Registration is first-wins and idempotent.

use std::sync::{Arc, OnceLock};

use async_trait::async_trait;

use crate::util::types::Message as AIMessage;

/// Cognitive engine injection surface.
///
/// Implementations (e.g. the Trinity daemon bridge) are async because they
/// fetch protocol and state from the engine.
#[async_trait]
pub trait CognitiveInjector: Send + Sync {
    /// Session-stable protocol text to prepend to the system prompt.
    ///
    /// The text should be byte-identical across turns so the system prompt
    /// prefix stays cacheable. `None` = no injection this turn.
    async fn cognitive_protocol(&self) -> Option<String>;

    /// Per-turn cognitive state without user-message context.
    ///
    /// Defaults to `cognitive_state_for("", "")`; implementors should override
    /// `cognitive_state_for` so the engine receives the real user message.
    async fn cognitive_state(&self) -> Option<String> {
        self.cognitive_state_for("", "").await
    }

    /// Per-turn cognitive state carrying the current user message and turn key.
    ///
    /// `turn_key` (e.g. the dialog turn id) lets the engine deduplicate work
    /// across the tool rounds of one user turn. `None` = no injection.
    async fn cognitive_state_for(&self, _user_message: &str, _turn_key: &str) -> Option<String> {
        self.cognitive_state().await
    }
}

static COGNITIVE_INJECTOR: OnceLock<Arc<dyn CognitiveInjector>> = OnceLock::new();

/// Register the global cognitive injector (first registration wins).
pub fn set_cognitive_injector(injector: Arc<dyn CognitiveInjector>) -> bool {
    COGNITIVE_INJECTOR.set(injector).is_ok()
}

/// Get the registered injector, or `None` when no engine is attached.
pub fn get_cognitive_injector() -> Option<&'static dyn CognitiveInjector> {
    COGNITIVE_INJECTOR.get().map(|injector| injector.as_ref())
}

/// Prepend the registered protocol to a resolved system prompt (no-op when
/// unset, empty, or failing).
pub(crate) async fn decorate_system_prompt(base: String) -> String {
    let Some(injector) = COGNITIVE_INJECTOR.get() else {
        return base;
    };
    match injector.cognitive_protocol().await {
        Some(protocol) if !protocol.trim().is_empty() => {
            let decorated = format!("{protocol}\n\n{base}");
            log::debug!(
                "[cognitive_injector] system prompt decorated: protocol={} bytes -> total={} bytes",
                protocol.len(),
                decorated.len()
            );
            decorated
        }
        _ => base,
    }
}

/// Prepend the registered per-turn state to the latest user message (no-op when
/// unset, empty, failing, or when the request has no user message).
///
/// The state text carries its own block header (engine side); this consumer
/// only concatenates it as a prefix.
pub(crate) async fn decorate_latest_user_message(
    mut messages: Vec<AIMessage>,
    user_message: &str,
    turn_key: &str,
) -> Vec<AIMessage> {
    let Some(injector) = COGNITIVE_INJECTOR.get() else {
        return messages;
    };
    let Some(state) = injector.cognitive_state_for(user_message, turn_key).await else {
        return messages;
    };
    if state.trim().is_empty() {
        return messages;
    }

    let block = format!("{state}\n\n");
    for message in messages.iter_mut().rev() {
        if message.role == "user" {
            let content = message.content.take().unwrap_or_default();
            message.content = Some(format!("{block}{content}"));
            log::debug!(
                "[cognitive_injector] user message decorated: state={} bytes",
                state.len()
            );
            break;
        }
    }
    messages
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The channels must be exact no-ops while unregistered so the execution
    /// engine behaves identically to upstream.
    #[tokio::test]
    async fn channels_are_noop_when_unregistered() {
        if COGNITIVE_INJECTOR.get().is_some() {
            // Another test registered an injector; skip rather than assert on
            // shared global state.
            return;
        }
        let base = "base system prompt".to_string();
        assert_eq!(decorate_system_prompt(base.clone()).await, base);

        let messages = vec![AIMessage::user("hello".to_string())];
        let decorated = decorate_latest_user_message(messages, "hello", "turn-1").await;
        assert_eq!(decorated.len(), 1);
        assert_eq!(decorated[0].content.as_deref(), Some("hello"));
    }
}
