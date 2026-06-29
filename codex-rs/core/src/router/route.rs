//! Core routing decision logic. Pure functions, no I/O.
//!
//! Ported from `aimux-ts-prototype/src/router/index.ts` and `fallback.ts`.

use super::complexity::estimate_complexity;
use super::store::LearningStore;
use super::store::OVERRIDE_CONFIDENCE_THRESHOLD;
use super::task_type::detect_task_type;
use super::types::ProviderId;
use super::types::RouteDecision;
use super::types::TaskType;

/// Inputs for a routing decision.
pub struct RouteContext<'a> {
    /// Providers currently installed + usable (subset of config providers).
    pub available: &'a [ProviderId],
    /// Resolved priority order to try providers in (e.g. config order).
    pub priority: &'a [ProviderId],
    /// Persisted learning store (usage, success, overrides).
    pub store: &'a LearningStore,
}

/// Filter `priority` down to providers that are currently available,
/// preserving priority order.
fn available_in_priority_order(ctx: &RouteContext<'_>) -> Vec<ProviderId> {
    ctx.priority
        .iter()
        .filter(|id| ctx.available.iter().any(|a| a == *id))
        .cloned()
        .collect()
}

/// If the user has manually overridden routing for this task type enough times
/// (>= `OVERRIDE_CONFIDENCE_THRESHOLD`) toward an available provider, return it.
/// When several providers qualify, the highest-confidence one wins.
fn learned_preference(
    task_type: TaskType,
    ctx: &RouteContext<'_>,
    available_in_priority: &[ProviderId],
) -> Option<ProviderId> {
    let mut best: Option<ProviderId> = None;
    let mut best_score: u64 = 0;
    for id in available_in_priority {
        let score = ctx.store.override_score(task_type, id);
        if score < OVERRIDE_CONFIDENCE_THRESHOLD {
            continue;
        }
        if score > best_score {
            best_score = score;
            best = Some(id.clone());
        }
    }
    best
}

/// Decide which provider should handle `prompt`.
///
/// Order of logic:
///   1. Learned preference — a confident manual override for the task type.
///   2. Otherwise the first available provider in priority order.
///
/// Fallbacks are the remaining available providers in priority order.
pub fn route(prompt: &str, ctx: &RouteContext<'_>) -> RouteDecision {
    let task_type = detect_task_type(prompt);
    let complexity = estimate_complexity(prompt);
    let in_priority = available_in_priority_order(ctx);

    let learned = learned_preference(task_type, ctx, &in_priority);

    let (provider, reason): (Option<ProviderId>, String) = match learned {
        Some(p) => (
            Some(p),
            format!(
                "learned preference for {} (manual overrides >= {OVERRIDE_CONFIDENCE_THRESHOLD})",
                task_type.as_str()
            ),
        ),
        None => match in_priority.first() {
            Some(p) => (
                Some(p.clone()),
                format!(
                    "first available provider in priority order for {}/{}",
                    task_type.as_str(),
                    complexity.as_str()
                ),
            ),
            None => (None, "no providers available".to_string()),
        },
    };

    // Defensive fallback: priority filtering left nothing but `available` still
    // has entries (e.g. priority omitted a provider).
    let (provider, reason) = match provider {
        Some(p) => (Some(p), reason),
        None => match ctx.available.first() {
            Some(p) => (
                Some(p.clone()),
                format!(
                    "fallback to first available provider for {}/{}",
                    task_type.as_str(),
                    complexity.as_str()
                ),
            ),
            None => (None, reason),
        },
    };

    // Final safety net mirrors the TS `chosen ?? ctx.priority[0] ?? "claude"`.
    let chosen = provider
        .or_else(|| ctx.priority.first().cloned())
        .unwrap_or_else(|| "openai".to_string());

    let fallbacks: Vec<ProviderId> = in_priority
        .into_iter()
        .filter(|id| id != &chosen)
        .collect();

    RouteDecision {
        provider: chosen,
        task_type,
        complexity,
        reason,
        fallbacks,
    }
}

/// Pick the next fallback provider for a decision that is not in `exclude`.
///
/// Used after the primary provider (or a prior fallback) hits a 429: the caller
/// passes the providers already attempted in `exclude`, and we return the next
/// untried entry from `decision.fallbacks`. Returns `None` when exhausted.
pub fn next_fallback(decision: &RouteDecision, exclude: &[ProviderId]) -> Option<ProviderId> {
    decision
        .fallbacks
        .iter()
        .find(|id| **id != decision.provider && !exclude.iter().any(|e| e == *id))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::router::types::Complexity;
    use tempfile::TempDir;

    fn ids(v: &[&str]) -> Vec<ProviderId> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn empty_store() -> (TempDir, LearningStore) {
        let dir = TempDir::new().expect("temp dir");
        let store = LearningStore::load_from_codex_home(dir.path());
        (dir, store)
    }

    #[test]
    fn picks_first_in_priority_when_no_learning() {
        let (_d, store) = empty_store();
        let available = ids(&["openai", "anthropic"]);
        let priority = ids(&["anthropic", "openai"]);
        let ctx = RouteContext {
            available: &available,
            priority: &priority,
            store: &store,
        };
        let decision = route("fix this rust function", &ctx);
        assert_eq!(decision.provider, "anthropic");
        assert_eq!(decision.task_type, TaskType::Code);
        assert_eq!(decision.fallbacks, ids(&["openai"]));
    }

    #[test]
    fn learned_preference_overrides_priority() {
        let (_d, mut store) = empty_store();
        for _ in 0..3 {
            store.record_override(TaskType::Code, "openai");
        }
        let available = ids(&["openai", "anthropic"]);
        let priority = ids(&["anthropic", "openai"]);
        let ctx = RouteContext {
            available: &available,
            priority: &priority,
            store: &store,
        };
        let decision = route("debug this code", &ctx);
        assert_eq!(decision.provider, "openai");
        assert!(decision.reason.contains("learned preference"));
    }

    #[test]
    fn learned_preference_ignored_when_provider_unavailable() {
        let (_d, mut store) = empty_store();
        for _ in 0..5 {
            store.record_override(TaskType::Code, "anthropic");
        }
        // anthropic learned but NOT available => falls back to priority.
        let available = ids(&["openai"]);
        let priority = ids(&["openai", "anthropic"]);
        let ctx = RouteContext {
            available: &available,
            priority: &priority,
            store: &store,
        };
        let decision = route("refactor this function", &ctx);
        assert_eq!(decision.provider, "openai");
    }

    #[test]
    fn below_threshold_does_not_override() {
        let (_d, mut store) = empty_store();
        store.record_override(TaskType::Code, "openai");
        store.record_override(TaskType::Code, "openai");
        let available = ids(&["anthropic", "openai"]);
        let priority = ids(&["anthropic", "openai"]);
        let ctx = RouteContext {
            available: &available,
            priority: &priority,
            store: &store,
        };
        let decision = route("debug this", &ctx);
        assert_eq!(decision.provider, "anthropic");
    }

    #[test]
    fn defensive_fallback_to_available_when_priority_empty() {
        let (_d, store) = empty_store();
        let available = ids(&["openai"]);
        let priority: Vec<ProviderId> = vec![];
        let ctx = RouteContext {
            available: &available,
            priority: &priority,
            store: &store,
        };
        let decision = route("hello", &ctx);
        assert_eq!(decision.provider, "openai");
        assert!(decision.reason.contains("fallback to first available"));
    }

    #[test]
    fn complexity_is_reported() {
        let (_d, store) = empty_store();
        let available = ids(&["openai"]);
        let priority = ids(&["openai"]);
        let ctx = RouteContext {
            available: &available,
            priority: &priority,
            store: &store,
        };
        let decision = route("architect a distributed system", &ctx);
        assert_eq!(decision.complexity, Complexity::Complex);
    }

    #[test]
    fn next_fallback_walks_then_exhausts() {
        let decision = RouteDecision {
            provider: "anthropic".to_string(),
            task_type: TaskType::Code,
            complexity: Complexity::Moderate,
            reason: String::new(),
            fallbacks: ids(&["openai", "gemini"]),
        };
        let first = next_fallback(&decision, &[]);
        assert_eq!(first, Some("openai".to_string()));
        let second = next_fallback(&decision, &ids(&["openai"]));
        assert_eq!(second, Some("gemini".to_string()));
        let none = next_fallback(&decision, &ids(&["openai", "gemini"]));
        assert_eq!(none, None);
    }
}
