//! Unicode / case / whitespace normalization primitives (§6.2 matching order).
//!
//! Pure functions, no I/O. The full pipeline (Unicode normalization, explicit
//! alias table, Unicode word boundaries, field-role context) is assembled in
//! M1; this module provides the leaf transforms.

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
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::normalize_text;

    #[test]
    fn collapses_whitespace_and_lowercases() {
        assert_eq!(normalize_text("  Don   ZAGIER "), "don zagier");
    }

    #[test]
    fn preserves_non_ascii() {
        assert_eq!(normalize_text("André  Weil"), "andré weil");
    }
}
