//! Compile-time word lists: `include_str!` an asset file, parse once.
//!
//! Format: comma-separated values per line; blank and `#` lines ignored. Keeps
//! editable vocabulary in `assets/` while staying offline/WASM-safe (embedded at
//! build time, no runtime I/O).

/// Parse a comma-separated, `#`-commented list. Input is `&'static` (from
/// `include_str!`), so the entries are too.
pub fn parse(raw: &'static str) -> Vec<&'static str> {
    raw.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .flat_map(|line| line.split(','))
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_comments_and_blanks_and_splits_commas() {
        let raw = "# header\n\nalpha, beta ,gamma\n# note\ndelta\n";
        assert_eq!(parse(raw), vec!["alpha", "beta", "gamma", "delta"]);
    }

    #[test]
    fn empty_and_comment_only_yield_nothing() {
        assert!(parse("# only a comment\n\n  \n").is_empty());
    }
}
