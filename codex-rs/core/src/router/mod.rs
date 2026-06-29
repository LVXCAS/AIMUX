//! AIMUX model **router + learning** module.
//!
//! Ported from the TypeScript prototype at `aimux-ts-prototype/src/router`
//! and `aimux-ts-prototype/src/learning`.
//!
//! This module is intentionally self-contained and side-effect-light:
//!
//! * [`complexity`] / [`task_type`] are pure prompt-classification heuristics.
//! * [`route`] is the pure routing decision (learned preference wins over
//!   config priority order; falls back gracefully).
//! * [`store`] is the JSON-persisted learning store (usage / success /
//!   override confidence), keyed by task-type and provider id, written to
//!   `<codex_home>/router_state.json`.
//!
//! ## Wiring status (CONSERVATIVE)
//!
//! The router is gated behind the `router_enabled` config flag, which defaults
//! to `false`. When disabled (the default), existing model-provider resolution
//! behavior is **completely unchanged**.
//!
//! See [`resolve_provider_override`] for the single, side-effect-free hook
//! point intended for use at the provider-resolution site in
//! `config/mod.rs` (~the `model_provider_id` resolution) or
//! `thread_manager::build_models_manager`.
//!
//! TODO(live-hook): The live call site is not yet wired so that the default
//! build behavior stays byte-for-byte identical and the change stays
//! non-invasive. To enable end-to-end routing, call
//! [`resolve_provider_override`] at the provider-resolution site, passing the
//! turn's prompt text and the configured provider ids, and use its return
//! value to override `model_provider_id` when `router_enabled` is true.
//! Recording (`record_run` / `record_override`) should be invoked from the
//! turn lifecycle and the `/route` override command respectively.

pub mod complexity;
pub mod route;
pub mod store;
pub mod task_type;
pub mod types;

pub use complexity::estimate_complexity;
pub use route::RouteContext;
pub use route::next_fallback;
pub use route::route;
pub use store::LearningStore;
pub use store::OVERRIDE_CONFIDENCE_THRESHOLD;
pub use store::ROUTER_STATE_FILENAME;
pub use store::RouterState;
pub use task_type::detect_task_type;
pub use types::Complexity;
pub use types::ProviderId;
pub use types::RouteDecision;
pub use types::TaskType;

/// Conservative, side-effect-free entry point for the provider-resolution hook.
///
/// Returns `Some(provider_id)` only when ALL of the following hold:
///   * `router_enabled` is `true`,
///   * at least one provider is available,
///   * the routing decision differs from `default_provider_id`.
///
/// Otherwise returns `None`, meaning "keep the existing resolved provider".
/// This makes it safe to call at the resolution site without changing default
/// behavior: when the flag is off (the default), it is a no-op.
///
/// `priority` should be the configured provider order; when empty it defaults
/// to `available`. `store` carries learned preferences.
pub fn resolve_provider_override(
    router_enabled: bool,
    prompt: &str,
    default_provider_id: &str,
    available: &[ProviderId],
    priority: &[ProviderId],
    store: &LearningStore,
) -> Option<ProviderId> {
    if !router_enabled {
        return None;
    }
    if available.is_empty() {
        return None;
    }
    let effective_priority: Vec<ProviderId> = if priority.is_empty() {
        available.to_vec()
    } else {
        priority.to_vec()
    };
    let ctx = RouteContext {
        available,
        priority: &effective_priority,
        store,
    };
    let decision = route(prompt, &ctx);
    if decision.provider == default_provider_id {
        None
    } else {
        Some(decision.provider)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn ids(v: &[&str]) -> Vec<ProviderId> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn disabled_flag_is_noop() {
        let dir = TempDir::new().unwrap();
        let mut store = LearningStore::load_from_codex_home(dir.path());
        for _ in 0..5 {
            store.record_override(TaskType::Code, "anthropic");
        }
        let available = ids(&["openai", "anthropic"]);
        let priority = ids(&["openai", "anthropic"]);
        let out = resolve_provider_override(
            false,
            "debug this rust function",
            "openai",
            &available,
            &priority,
            &store,
        );
        assert_eq!(out, None);
    }

    #[test]
    fn enabled_returns_override_when_different() {
        let dir = TempDir::new().unwrap();
        let mut store = LearningStore::load_from_codex_home(dir.path());
        for _ in 0..3 {
            store.record_override(TaskType::Code, "anthropic");
        }
        let available = ids(&["openai", "anthropic"]);
        let priority = ids(&["openai", "anthropic"]);
        let out = resolve_provider_override(
            true,
            "debug this rust function",
            "openai",
            &available,
            &priority,
            &store,
        );
        assert_eq!(out, Some("anthropic".to_string()));
    }

    #[test]
    fn enabled_returns_none_when_same_as_default() {
        let dir = TempDir::new().unwrap();
        let store = LearningStore::load_from_codex_home(dir.path());
        let available = ids(&["openai", "anthropic"]);
        let priority = ids(&["openai", "anthropic"]);
        // No learning => routes to first in priority = openai = default.
        let out = resolve_provider_override(
            true,
            "debug this",
            "openai",
            &available,
            &priority,
            &store,
        );
        assert_eq!(out, None);
    }
}
