//! Pure, framework-agnostic word-wrapping.
//!
//! This exists because the content pane's scroll model (`app::clamp_content_scroll`,
//! `clamp_content_cursor_scroll`, the scrollbar, the windowing in
//! `view_model::build_content_view`) all need to agree on exactly how many terminal
//! rows a line will occupy once wrapped -- and `ui::draw_content` needs to actually
//! produce those same rows when rendering. Using one shared algorithm for both
//! "how many rows will this take" and "here are the rows" is what keeps them from
//! drifting apart the way `Paragraph::wrap()` did (see the fix for #71: wrapping via
//! a widget whose row count the rest of the app couldn't predict silently broke the
//! one-line-per-row invariant the scroll math relied on).

use std::ops::Range;
use unicode_width::UnicodeWidthChar;

/// Word-wraps `text` to `width` display columns (Unicode-aware -- e.g. CJK
/// characters count as two columns, matching how a terminal actually renders
/// them), returning the byte range of each wrapped row.
///
/// Always returns at least one row, even for empty text or `width == 0`, so
/// callers relying on "one line maps to >=1 row" (the scroll/clamp math) never
/// see a line silently disappear.
pub fn wrap_row_ranges(text: &str, width: usize) -> Vec<Range<usize>> {
    if text.is_empty() {
        return std::iter::once(0..0).collect();
    }
    if width == 0 {
        return std::iter::once(0..text.len()).collect();
    }

    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let n = chars.len();
    let byte_at = |idx: usize| -> usize {
        if idx < n {
            chars[idx].0
        } else {
            text.len()
        }
    };

    let mut rows = Vec::new();
    // Index into `chars` (not a byte offset) where the current row begins.
    let mut row_start = 0usize;
    let mut col = 0usize;
    // Index into `chars` of the most recent whitespace char seen since
    // `row_start`, i.e. the fallback break point for the current row.
    let mut last_ws: Option<usize> = None;

    let mut i = 0;
    while i < n {
        let (_, ch) = chars[i];
        let ch_width = ch.width().unwrap_or(0);

        if col + ch_width > width {
            if i == row_start {
                // A single character wider than `width` on its own (e.g. a
                // CJK glyph in a near-zero-width pane): place it alone so
                // wrapping always makes forward progress.
                rows.push(byte_at(row_start)..byte_at(i + 1));
                row_start = i + 1;
            } else if ch.is_whitespace() {
                // The overflowing char is itself whitespace: everything
                // before it already fits exactly, so it's the best possible
                // break point -- more recent than anything recorded earlier.
                rows.push(byte_at(row_start)..byte_at(i));
                row_start = i + 1;
            } else if let Some(ws) = last_ws {
                rows.push(byte_at(row_start)..byte_at(ws));
                row_start = ws + 1;
            } else {
                // No whitespace anywhere in this row: hard-break mid-word.
                rows.push(byte_at(row_start)..byte_at(i));
                row_start = i;
            }
            i = row_start;
            col = 0;
            last_ws = None;
            continue;
        }

        if ch.is_whitespace() {
            last_ws = Some(i);
        }
        col += ch_width;
        i += 1;
    }
    // A break exactly at the last character (e.g. a line ending in the
    // whitespace or oversized char that triggered it) can leave `row_start`
    // pointing past the end with nothing left to show -- skip the trailing
    // push rather than emit a phantom empty row.
    if row_start < n {
        rows.push(byte_at(row_start)..text.len());
    }
    rows
}

/// The number of wrapped rows `text` occupies at `width` columns. See
/// `wrap_row_ranges`.
pub fn row_count(text: &str, width: usize) -> usize {
    wrap_row_ranges(text, width).len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(text: &str, width: usize) -> Vec<&str> {
        wrap_row_ranges(text, width)
            .into_iter()
            .map(|r| &text[r])
            .collect()
    }

    #[test]
    fn empty_text_is_one_row() {
        assert_eq!(rows("", 10), vec![""]);
        assert_eq!(row_count("", 10), 1);
    }

    #[test]
    fn zero_width_is_one_unbroken_row() {
        assert_eq!(rows("hello world", 0), vec!["hello world"]);
    }

    #[test]
    fn short_text_fits_on_one_row() {
        assert_eq!(rows("hello", 10), vec!["hello"]);
    }

    #[test]
    fn breaks_on_whitespace_dropping_the_space() {
        assert_eq!(rows("hello world", 5), vec!["hello", "world"]);
    }

    #[test]
    fn keeps_whole_words_together_when_they_fit() {
        // "hello world" is exactly 11 columns -- must stay on one row rather
        // than breaking at an earlier space than necessary.
        assert_eq!(rows("hello world foo", 11), vec!["hello world", "foo"]);
    }

    #[test]
    fn hard_breaks_a_single_word_longer_than_width() {
        assert_eq!(rows("ab cdefgh", 5), vec!["ab", "cdefg", "h"]);
    }

    #[test]
    fn hard_breaks_word_that_never_had_a_space() {
        assert_eq!(rows("aaaa", 2), vec!["aa", "aa"]);
    }

    #[test]
    fn cjk_wide_characters_count_as_two_columns() {
        // Each character here is width 2, so 3 chars == 6 columns exactly.
        assert_eq!(rows("日本語のテスト", 6), vec!["日本語", "のテス", "ト"]);
    }

    #[test]
    fn wide_char_that_alone_exceeds_width_is_isolated() {
        assert_eq!(rows("日", 1), vec!["日"]);
    }

    #[test]
    fn row_count_matches_number_of_ranges() {
        assert_eq!(row_count("x".repeat(200).as_str(), 19), 11);
    }

    #[test]
    fn multiple_consecutive_spaces_break_at_the_overflowing_one() {
        // Not load-bearing behavior, just documenting it: a run of spaces
        // that doesn't fit breaks at the first space that overflows, rather
        // than at the start of the run.
        let text = "aa    bb";
        let wrapped = rows(text, 4);
        assert_eq!(wrapped, vec!["aa  ", " bb"]);
    }
}
