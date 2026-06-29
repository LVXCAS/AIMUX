//! Persistent learning store for the AIMUX router.
//!
//! Ported from `aimux-ts-prototype/src/learning/index.ts` (the `Learning`
//! class) and `src/core/store.ts` (load/save). Persists usage / success /
//! override stats as JSON under the AIMUX (Codex) home dir in
//! `router_state.json`.
//!
//! State shape (keyed by task-type string, then provider id):
//!   usage[taskType][provider]     -> run count (u64)
//!   success[taskType][provider]   -> { ok, total }
//!   overrides[taskType][provider] -> manual-override confidence (u64)
//!
//! Once override confidence for a (taskType, provider) reaches
//! `OVERRIDE_CONFIDENCE_THRESHOLD`, that provider becomes the learned
//! preference for the task type (highest-confidence wins).

use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

use super::types::ProviderId;
use super::types::TaskType;

/// Number of manual overrides toward a provider (for a task type) required
/// before it becomes the learned preference. Mirrors
/// `OVERRIDE_CONFIDENCE_THRESHOLD` in the TS prototype.
pub const OVERRIDE_CONFIDENCE_THRESHOLD: u64 = 3;

/// File name under the Codex/AIMUX home dir for persisted router state.
pub const ROUTER_STATE_FILENAME: &str = "router_state.json";

/// Success ledger for a (taskType, provider).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuccessCell {
    pub ok: u64,
    pub total: u64,
}

/// Per-task-type provider map. Keyed by provider id.
type ProviderMap<T> = BTreeMap<ProviderId, T>;

/// Top-level keyed-by-task-type map. We persist the task type as its lowercase
/// string (e.g. `"code"`) for forward-compatible, human-readable JSON.
type TaskMap<T> = BTreeMap<String, ProviderMap<T>>;

/// The full persisted router state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouterState {
    #[serde(default)]
    pub usage: TaskMap<u64>,
    #[serde(default)]
    pub success: TaskMap<SuccessCell>,
    #[serde(default)]
    pub overrides: TaskMap<u64>,
}

/// A learning store backed by an on-disk JSON file. Persistence is best-effort:
/// recording failures are swallowed so the router never breaks a turn.
#[derive(Debug, Clone)]
pub struct LearningStore {
    path: PathBuf,
    state: RouterState,
}

impl LearningStore {
    /// Load the store from an explicit file path. Missing / unreadable /
    /// malformed files yield a fresh empty state (never an error), matching the
    /// best-effort semantics of the TS prototype.
    pub fn load_from_path(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let state = std::fs::read_to_string(&path)
            .ok()
            .and_then(|contents| serde_json::from_str::<RouterState>(&contents).ok())
            .unwrap_or_default();
        Self { path, state }
    }

    /// Load the store from `<codex_home>/router_state.json`.
    pub fn load_from_codex_home(codex_home: &Path) -> Self {
        Self::load_from_path(codex_home.join(ROUTER_STATE_FILENAME))
    }

    /// Borrow the underlying state (read-only).
    pub fn state(&self) -> &RouterState {
        &self.state
    }

    /// The path this store persists to.
    pub fn path(&self) -> &Path {
        &self.path
    }

    // ---- recording ------------------------------------------------------

    /// Bump usage count and the success ledger for a (taskType, provider) run.
    pub fn record_run(&mut self, task_type: TaskType, provider: &str, ok: bool) {
        let key = task_type.as_str().to_string();

        let usage_row = self.state.usage.entry(key.clone()).or_default();
        *usage_row.entry(provider.to_string()).or_insert(0) += 1;

        let success_row = self.state.success.entry(key).or_default();
        let cell = success_row.entry(provider.to_string()).or_default();
        cell.total += 1;
        if ok {
            cell.ok += 1;
        }

        self.persist();
    }

    /// Bump override confidence for a (taskType, provider).
    pub fn record_override(&mut self, task_type: TaskType, provider: &str) {
        let row = self
            .state
            .overrides
            .entry(task_type.as_str().to_string())
            .or_default();
        *row.entry(provider.to_string()).or_insert(0) += 1;
        self.persist();
    }

    // ---- queries --------------------------------------------------------

    /// The learned preferred provider for a task type: the provider with the
    /// highest override confidence that is at or above the threshold. Returns
    /// `None` when nothing has crossed the threshold.
    pub fn preferred_for(&self, task_type: TaskType) -> Option<ProviderId> {
        let row = self.state.overrides.get(task_type.as_str())?;
        let mut best: Option<ProviderId> = None;
        let mut best_score: u64 = 0;
        for (provider, &score) in row {
            if score >= OVERRIDE_CONFIDENCE_THRESHOLD && score > best_score {
                best = Some(provider.clone());
                best_score = score;
            }
        }
        best
    }

    /// Override confidence for a (taskType, provider). 0 when unseen.
    pub fn override_score(&self, task_type: TaskType, provider: &str) -> u64 {
        self.state
            .overrides
            .get(task_type.as_str())
            .and_then(|row| row.get(provider))
            .copied()
            .unwrap_or(0)
    }

    /// Usage count for a (taskType, provider). 0 when unseen.
    pub fn usage_count(&self, task_type: TaskType, provider: &str) -> u64 {
        self.state
            .usage
            .get(task_type.as_str())
            .and_then(|row| row.get(provider))
            .copied()
            .unwrap_or(0)
    }

    /// Success rate in [0.0, 1.0] for a (taskType, provider). 0.0 when no runs.
    pub fn success_rate(&self, task_type: TaskType, provider: &str) -> f64 {
        match self
            .state
            .success
            .get(task_type.as_str())
            .and_then(|row| row.get(provider))
        {
            Some(cell) if cell.total > 0 => cell.ok as f64 / cell.total as f64,
            _ => 0.0,
        }
    }

    // ---- internals ------------------------------------------------------

    /// Persist to disk. Best-effort: errors are swallowed so the router never
    /// throws across the module boundary (matches TS `persist()`).
    fn persist(&self) {
        let Ok(contents) = serde_json::to_string_pretty(&self.state) else {
            return;
        };
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&self.path, contents);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn tmp_store() -> (TempDir, LearningStore) {
        let dir = TempDir::new().expect("temp dir");
        let store = LearningStore::load_from_codex_home(dir.path());
        (dir, store)
    }

    #[test]
    fn empty_store_has_no_preference() {
        let (_d, store) = tmp_store();
        assert_eq!(store.preferred_for(TaskType::Code), None);
        assert_eq!(store.success_rate(TaskType::Code, "openai"), 0.0);
        assert_eq!(store.usage_count(TaskType::Code, "openai"), 0);
    }

    #[test]
    fn records_run_and_success_rate() {
        let (_d, mut store) = tmp_store();
        store.record_run(TaskType::Code, "openai", true);
        store.record_run(TaskType::Code, "openai", false);
        assert_eq!(store.usage_count(TaskType::Code, "openai"), 2);
        assert!((store.success_rate(TaskType::Code, "openai") - 0.5).abs() < 1e-9);
    }

    #[test]
    fn override_threshold_creates_preference() {
        let (_d, mut store) = tmp_store();
        // Below threshold => no preference.
        store.record_override(TaskType::Code, "anthropic");
        store.record_override(TaskType::Code, "anthropic");
        assert_eq!(store.preferred_for(TaskType::Code), None);
        // Crossing threshold (3) => preference.
        store.record_override(TaskType::Code, "anthropic");
        assert_eq!(
            store.preferred_for(TaskType::Code),
            Some("anthropic".to_string())
        );
    }

    #[test]
    fn highest_confidence_wins() {
        let (_d, mut store) = tmp_store();
        for _ in 0..3 {
            store.record_override(TaskType::Analysis, "openai");
        }
        for _ in 0..5 {
            store.record_override(TaskType::Analysis, "anthropic");
        }
        assert_eq!(
            store.preferred_for(TaskType::Analysis),
            Some("anthropic".to_string())
        );
    }

    #[test]
    fn persists_and_reloads() {
        let dir = TempDir::new().expect("temp dir");
        {
            let mut store = LearningStore::load_from_codex_home(dir.path());
            for _ in 0..3 {
                store.record_override(TaskType::Writing, "openai");
            }
            store.record_run(TaskType::Writing, "openai", true);
        }
        // Fresh load from the same dir sees persisted data.
        let reloaded = LearningStore::load_from_codex_home(dir.path());
        assert_eq!(
            reloaded.preferred_for(TaskType::Writing),
            Some("openai".to_string())
        );
        assert_eq!(reloaded.usage_count(TaskType::Writing, "openai"), 1);
    }

    #[test]
    fn malformed_file_yields_empty_state() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join(ROUTER_STATE_FILENAME);
        std::fs::write(&path, "this is not json").unwrap();
        let store = LearningStore::load_from_path(&path);
        assert_eq!(store.state(), &RouterState::default());
    }
}
