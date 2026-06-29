//! Shared types for the AIMUX model router + learning module.
//!
//! Ported from the TypeScript prototype at
//! `aimux-ts-prototype/src/core/types.ts` and `src/router`.
//!
//! These are pure data types with no I/O.

use serde::Deserialize;
use serde::Serialize;

/// Coarse prompt complexity estimate. Used as a routing signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Complexity {
    Simple,
    Moderate,
    Complex,
}

impl Complexity {
    pub fn as_str(self) -> &'static str {
        match self {
            Complexity::Simple => "simple",
            Complexity::Moderate => "moderate",
            Complexity::Complex => "complex",
        }
    }
}

/// Detected task category. `General` is the fallback when nothing matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskType {
    Writing,
    Analysis,
    Code,
    Summary,
    Brainstorm,
    Translation,
    General,
}

impl TaskType {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskType::Writing => "writing",
            TaskType::Analysis => "analysis",
            TaskType::Code => "code",
            TaskType::Summary => "summary",
            TaskType::Brainstorm => "brainstorm",
            TaskType::Translation => "translation",
            TaskType::General => "general",
        }
    }

    /// All task types, in a stable order. Mirrors `TASK_TYPES` in the
    /// TS prototype.
    pub const ALL: [TaskType; 7] = [
        TaskType::Writing,
        TaskType::Analysis,
        TaskType::Code,
        TaskType::Summary,
        TaskType::Brainstorm,
        TaskType::Translation,
        TaskType::General,
    ];
}

/// A model-provider id. In the Codex fork this is the key into the
/// `model_providers` map (e.g. `"openai"`, `"anthropic"`).
pub type ProviderId = String;

/// The outcome of a routing decision. Mirrors `RouteDecision` in the TS
/// prototype.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteDecision {
    /// The chosen provider id.
    pub provider: ProviderId,
    /// Detected task type.
    pub task_type: TaskType,
    /// Estimated complexity.
    pub complexity: Complexity,
    /// Human-readable reason for the choice (useful for logging / debug).
    pub reason: String,
    /// Remaining available providers in priority order, for fallback after a
    /// rate-limit (429) or failure.
    pub fallbacks: Vec<ProviderId>,
}
