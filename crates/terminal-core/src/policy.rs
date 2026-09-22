//! Resource limits and sanitisation for untrusted terminal output.

/// Per-session resource limits. Defaults follow the technical specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Maximum retained scrollback lines.
    pub scrollback_lines: usize,
    /// Accounted history budget in bytes; the first of the two limits wins.
    pub history_bytes: usize,
    /// Maximum title length in bytes after sanitisation.
    pub max_title_bytes: usize,
    /// Maximum literal search results.
    pub max_search_results: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            scrollback_lines: 10_000,
            history_bytes: 32 * 1024 * 1024,
            max_title_bytes: 4 * 1024,
            max_search_results: 10_000,
        }
    }
}

impl Limits {
    /// Approximate accounted bytes per retained cell (engine cell plus row overhead).
    pub const BYTES_PER_CELL: usize = 32;

    /// Effective scrollback lines for a given column count, honouring both
    /// the line cap and the byte budget.
    pub fn effective_scrollback(&self, columns: usize) -> usize {
        let per_line = columns.max(1) * Self::BYTES_PER_CELL;
        let by_bytes = self.history_bytes / per_line;
        self.scrollback_lines.min(by_bytes)
    }
}

/// Strip control characters from an untrusted title and bound its length on
/// a character boundary. Titles are displayed as plain text only.
pub fn sanitize_title(raw: &str, max_bytes: usize) -> String {
    let mut out = String::with_capacity(raw.len().min(max_bytes));
    for ch in raw
        .chars()
        .filter(|c| !c.is_control() && !is_bidi_control(*c))
    {
        if out.len() + ch.len_utf8() > max_bytes {
            break;
        }
        out.push(ch);
    }
    out
}

/// Bidirectional overrides can make a title display differently from its bytes.
fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_strips_controls_and_bidi() {
        assert_eq!(sanitize_title("a\x07b\x1b[31mc\u{202E}d", 64), "ab[31mcd");
    }

    #[test]
    fn title_is_bounded_on_char_boundary() {
        let title = sanitize_title(&"é".repeat(10), 5);
        assert_eq!(title, "éé");
    }

    #[test]
    fn scrollback_budget_first_limit_wins() {
        let limits = Limits::default();
        assert_eq!(limits.effective_scrollback(80), 10_000);
        let tight = Limits {
            history_bytes: 80 * 32 * 100,
            ..limits
        };
        assert_eq!(tight.effective_scrollback(80), 100);
    }
}
