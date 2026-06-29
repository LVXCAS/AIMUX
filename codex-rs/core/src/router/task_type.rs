//! Pure heuristic task-type detection for routing. No I/O.
//!
//! Ported from `aimux-ts-prototype/src/router/taskType.ts`.

use super::types::TaskType;

/// Ordered keyword groups. The first group with a match wins, so groups are
/// ranked from most-specific intent to least. `General` is the fallback.
const KEYWORD_GROUPS: &[(TaskType, &[&str])] = &[
    (
        TaskType::Translation,
        &[
            "translate",
            "translation",
            "in spanish",
            "in french",
            "in german",
            "in japanese",
            "in chinese",
            "into english",
            "to english",
            "localize",
            "localise",
        ],
    ),
    (
        TaskType::Code,
        &[
            "code",
            "function",
            "bug",
            "debug",
            "refactor",
            "implement",
            "compile",
            "stack trace",
            "stacktrace",
            "typescript",
            "javascript",
            "python",
            "rust",
            "golang",
            "regex",
            "api",
            "endpoint",
            "class ",
            "method",
            "unit test",
            "snippet",
            "syntax",
            "program",
        ],
    ),
    (
        TaskType::Summary,
        &[
            "summarize",
            "summarise",
            "summary",
            "tldr",
            "tl;dr",
            "key points",
            "key takeaways",
            "in short",
            "recap",
            "condense",
            "shorten",
        ],
    ),
    (
        TaskType::Brainstorm,
        &[
            "brainstorm",
            "ideas",
            "idea for",
            "name ideas",
            "suggest some",
            "give me options",
            "alternatives",
            "what could",
            "ways to",
            "list of ideas",
        ],
    ),
    (
        TaskType::Analysis,
        &[
            "analyze",
            "analyse",
            "analysis",
            "compare",
            "comparison",
            "evaluate",
            "assess",
            "pros and cons",
            "trade-off",
            "tradeoff",
            "why does",
            "why is",
            "explain why",
            "investigate",
            "root cause",
            "reasoning",
        ],
    ),
    (
        TaskType::Writing,
        &[
            "write",
            "draft",
            "compose",
            "essay",
            "article",
            "blog",
            "email",
            "letter",
            "story",
            "poem",
            "copy for",
            "rewrite",
            "paraphrase",
            "proofread",
            "edit this",
        ],
    ),
];

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

/// Detect the task category of a prompt via keyword/intent heuristics.
/// Returns `TaskType::General` when no group matches. Pure function.
pub fn detect_task_type(prompt: &str) -> TaskType {
    let text = prompt.to_lowercase();
    for (task_type, keywords) in KEYWORD_GROUPS {
        if contains_any(&text, keywords) {
            return *task_type;
        }
    }
    TaskType::General
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translation_first() {
        assert_eq!(
            detect_task_type("translate this to english"),
            TaskType::Translation
        );
    }

    #[test]
    fn code_detection() {
        assert_eq!(detect_task_type("fix this rust function"), TaskType::Code);
        assert_eq!(
            detect_task_type("write a python program"),
            // "python"/"program" are in the code group, which is ranked
            // above writing, so code wins.
            TaskType::Code
        );
    }

    #[test]
    fn summary_detection() {
        assert_eq!(
            detect_task_type("give me a tldr of this"),
            TaskType::Summary
        );
    }

    #[test]
    fn brainstorm_detection() {
        assert_eq!(
            detect_task_type("brainstorm some product names"),
            TaskType::Brainstorm
        );
    }

    #[test]
    fn analysis_detection() {
        assert_eq!(
            detect_task_type("analyze the pros and cons"),
            TaskType::Analysis
        );
    }

    #[test]
    fn writing_detection() {
        assert_eq!(detect_task_type("draft an essay"), TaskType::Writing);
    }

    #[test]
    fn general_fallback() {
        assert_eq!(detect_task_type("hello there friend"), TaskType::General);
    }

    #[test]
    fn ordering_translation_beats_code() {
        // "translate" (translation) appears before "code" group, so wins.
        assert_eq!(
            detect_task_type("translate this code snippet"),
            TaskType::Translation
        );
    }
}
