//! Unicode / case / whitespace normalization primitives (§6.2 matching order).
//!
//! Pure functions, no I/O. The full pipeline (Unicode normalization, explicit
//! alias table, Unicode word boundaries, field-role context) is assembled in
//! M1; this module provides the leaf transforms.

use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

/// Lowercase + collapse internal whitespace + trim. A pre-normalization step
/// used before alias and word-boundary matching.
pub fn normalize_text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev_space = true;
    for ch in input.chars() {
        if ch.is_whitespace() {
            if !prev_space {
                out.push(' ');
            }
            prev_space = true;
        } else {
            for lc in ch.to_lowercase() {
                out.push(lc);
            }
            prev_space = false;
        }
    }
    let trim_len = out.trim_end().len();
    out.truncate(trim_len);
    out
}

/// NFC-normalize `text`, then apply [`normalize_text`] (lowercase + whitespace
/// collapse + trim). NFC ensures combining sequences are in composed form so
/// that downstream alias and word-boundary matching compare canonically
/// equivalent strings. This covers steps 1–2 of the §6.2 pipeline; alias
/// resolution (step 3) and field-role context (step 5) live in the matcher
/// modules.
pub fn normalize_name(text: &str) -> String {
    let nfc: String = text.nfc().collect();
    normalize_text(&nfc)
}

/// Split `text` into Unicode word boundaries, returning each word as an owned
/// `String`. Case-preserving — callers normalize case separately via
/// [`normalize_name`] when needed (step 4 of the §6.2 pipeline, after NFC and
/// alias resolution). Returns an empty `Vec` for empty input.
pub fn word_boundaries(text: &str) -> Vec<String> {
    text.unicode_words().map(String::from).collect()
}

/// Word-boundary-aware phrase match: returns `true` if `phrase` appears in
/// `text` bounded by non-alphanumeric characters or string boundaries on both
/// sides. For multi-word phrases, the entire phrase must appear as a substring
/// (whitespace already makes it distinctive). For single tokens, this prevents
/// partial-word hits (e.g. "free" inside "freedom", "sso" inside "bossom").
///
/// Both `text` and `phrase` should be pre-normalized (lowercase) by the caller.
pub fn contains_phrase(text: &str, phrase: &str) -> bool {
    if phrase.is_empty() {
        return false;
    }
    if phrase.contains(char::is_whitespace) {
        return text.contains(phrase);
    }
    let mut search_from = 0;
    while let Some(rel) = text[search_from..].find(phrase) {
        let start = search_from + rel;
        let end = start + phrase.len();
        let before_ok = start == 0
            || text[..start]
                .chars()
                .next_back()
                .is_none_or(|c| !c.is_alphanumeric());
        let after_ok = end >= text.len()
            || text[end..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_alphanumeric());
        if before_ok && after_ok {
            return true;
        }
        search_from = end;
        if search_from >= text.len() {
            break;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{normalize_name, normalize_text, word_boundaries};

    #[test]
    fn collapses_whitespace_and_lowercases() {
        assert_eq!(normalize_text("  Don   ZAGIER "), "don zagier");
    }

    #[test]
    fn preserves_non_ascii() {
        assert_eq!(normalize_text("André  Weil"), "andré weil");
    }

    #[test]
    fn normalize_name_nfc_lowercases_and_collapses() {
        assert_eq!(normalize_name("André  Weil"), "andré weil");
    }

    #[test]
    fn normalize_name_mixed_case_extra_whitespace() {
        assert_eq!(normalize_name("Don B.  ZAGIER"), "don b. zagier");
    }

    #[test]
    fn normalize_name_empty() {
        assert_eq!(normalize_name(""), "");
    }

    #[test]
    fn word_boundaries_basic() {
        assert_eq!(
            word_boundaries("Gross-Zagier formula"),
            ["Gross", "Zagier", "formula"]
        );
    }

    #[test]
    fn word_boundaries_empty() {
        assert!(word_boundaries("").is_empty());
    }

    #[test]
    fn word_boundaries_cjk() {
        let words = word_boundaries("陶哲轩 Terence Tao");
        assert!(words.contains(&"Terence".to_string()));
        assert!(words.contains(&"Tao".to_string()));
        assert!(words.iter().any(|w| w.contains('陶')));
    }
}
