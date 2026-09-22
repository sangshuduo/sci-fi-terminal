//! Bounded literal scrollback search.
//!
//! MVP search is literal (no regex) and capped at `Limits::max_search_results`.
//! Matches are reported in stable history coordinates so they survive
//! scrolling; a result is tied to the snapshot version it was computed from.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;

use crate::adapter::TerminalModel;
use crate::ids::SnapshotVersion;

/// One match on a single terminal row. `line` is in engine history
/// coordinates: 0 is the top visible row of the live screen, negative is history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchMatch {
    pub line: i32,
    pub start_col: u16,
    pub end_col: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    pub query: String,
    pub matches: Vec<SearchMatch>,
    /// True when the result cap was reached.
    pub truncated: bool,
    pub version: SnapshotVersion,
}

impl TerminalModel {
    /// Case-insensitive literal search over retained history and the screen.
    /// Matches do not span soft-wrapped rows in the MVP.
    pub fn search_literal(&self, query: &str, version: SnapshotVersion) -> SearchResult {
        let mut result = SearchResult {
            query: query.to_owned(),
            matches: Vec::new(),
            truncated: false,
            version,
        };
        let needle: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
        if needle.is_empty() {
            return result;
        }
        let cap = self.limits_max_results();
        let grid = self.term().grid();
        let top = -(grid.history_size() as i32);
        let bottom = grid.screen_lines() as i32;
        for line in top..bottom {
            let (chars, cols) = row_text(grid, line);
            for (start, end) in find_all(&chars, &needle) {
                if result.matches.len() >= cap {
                    result.truncated = true;
                    return result;
                }
                result.matches.push(SearchMatch {
                    line,
                    start_col: cols[start],
                    end_col: cols[end - 1],
                });
            }
        }
        result
    }

    /// Scroll so that the given history line is visible.
    pub fn reveal_line(&mut self, line: i32) {
        self.scroll_to_line(line);
    }
}

/// Lower-cased characters of one row plus the column each character starts in.
fn row_text(
    grid: &alacritty_terminal::Grid<alacritty_terminal::term::cell::Cell>,
    line: i32,
) -> (Vec<char>, Vec<u16>) {
    let row = &grid[Line(line)];
    let mut chars = Vec::with_capacity(grid.columns());
    let mut cols = Vec::with_capacity(grid.columns());
    for col in 0..grid.columns() {
        let cell = &row[Column(col)];
        if cell
            .flags
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
        {
            continue;
        }
        for lower in cell.c.to_lowercase() {
            chars.push(lower);
            cols.push(col as u16);
        }
    }
    (chars, cols)
}

/// Non-overlapping occurrences of `needle` in `haystack` as `[start, end)` indices.
fn find_all(haystack: &[char], needle: &[char]) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut index = 0;
    while index + needle.len() <= haystack.len() {
        if haystack[index..index + needle.len()] == *needle {
            found.push((index, index + needle.len()));
            index += needle.len();
        } else {
            index += 1;
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::find_all;

    #[test]
    fn finds_non_overlapping_matches() {
        let hay: Vec<char> = "aaaa".chars().collect();
        let needle: Vec<char> = "aa".chars().collect();
        assert_eq!(find_all(&hay, &needle), vec![(0, 2), (2, 4)]);
    }

    #[test]
    fn needle_longer_than_haystack_finds_nothing() {
        let hay: Vec<char> = "ab".chars().collect();
        let needle: Vec<char> = "abc".chars().collect();
        assert!(find_all(&hay, &needle).is_empty());
    }
}
