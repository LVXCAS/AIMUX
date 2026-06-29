//! Pure heuristic complexity estimation for routing. No I/O.
//!
//! Ported from `aimux-ts-prototype/src/router/complexity.ts`.

use super::types::Complexity;

/// Keywords that strongly suggest a multi-step / hard task.
const COMPLEX_KEYWORDS: &[&str] = &[
    "architect",
    "architecture",
    "refactor",
    "debug",
    "design",
    "implement",
    "migrate",
    "optimize",
    "performance",
    "concurrency",
    "race condition",
    "step by step",
    "step-by-step",
    "multi-step",
    "end to end",
    "end-to-end",
    "scalable",
    "distributed",
    "algorithm",
    "trade-off",
    "tradeoff",
    "compare and",
    "in depth",
    "in-depth",
];

/// Keywords that suggest a quick factual / lookup task.
const SIMPLE_KEYWORDS: &[&str] = &[
    "what is",
    "what's",
    "who is",
    "who's",
    "when is",
    "when did",
    "where is",
    "define",
    "definition of",
    "meaning of",
    "translate",
    "convert",
    "list ",
    "how do you spell",
];

fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

/// Estimate the coarse complexity of a prompt using length and keyword
/// heuristics. Pure function; safe to call repeatedly.
pub fn estimate_complexity(prompt: &str) -> Complexity {
    let text = prompt.to_lowercase();
    let text = text.trim();
    if text.is_empty() {
        return Complexity::Simple;
    }

    let words = word_count(text);
    let has_complex_keyword = contains_any(text, COMPLEX_KEYWORDS);
    let has_simple_keyword = contains_any(text, SIMPLE_KEYWORDS);

    // Strong signals win first.
    if has_complex_keyword {
        return Complexity::Complex;
    }

    // Long prompts imply a lot of context / a big ask.
    if words >= 80 {
        return Complexity::Complex;
    }

    // Short and clearly a lookup => simple.
    if has_simple_keyword && words <= 25 {
        return Complexity::Simple;
    }

    // Very short prompts default to simple.
    if words <= 8 {
        return Complexity::Simple;
    }

    Complexity::Moderate
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_simple() {
        assert_eq!(estimate_complexity(""), Complexity::Simple);
        assert_eq!(estimate_complexity("   "), Complexity::Simple);
    }

    #[test]
    fn complex_keyword_wins() {
        assert_eq!(
            estimate_complexity("please refactor this module"),
            Complexity::Complex
        );
        assert_eq!(
            estimate_complexity("Help me debug a race condition"),
            Complexity::Complex
        );
        // Complex keyword beats short length.
        assert_eq!(estimate_complexity("optimize"), Complexity::Complex);
    }

    #[test]
    fn long_prompt_is_complex() {
        let prompt = "word ".repeat(80);
        assert_eq!(estimate_complexity(&prompt), Complexity::Complex);
    }

    #[test]
    fn simple_lookup() {
        assert_eq!(
            estimate_complexity("what is the capital of France"),
            Complexity::Simple
        );
        assert_eq!(estimate_complexity("define entropy"), Complexity::Simple);
    }

    #[test]
    fn very_short_is_simple() {
        assert_eq!(estimate_complexity("hello there"), Complexity::Simple);
    }

    #[test]
    fn medium_length_is_moderate() {
        // ~12 words, no keywords => moderate.
        let prompt = "Please tell me a little about the history of the city over time";
        assert_eq!(estimate_complexity(prompt), Complexity::Moderate);
    }

    #[test]
    fn simple_keyword_but_long_is_moderate() {
        // "list " keyword but > 25 words => not forced simple, falls through.
        let prompt = format!("list {}", "item ".repeat(30));
        assert_eq!(estimate_complexity(&prompt), Complexity::Moderate);
    }
}
