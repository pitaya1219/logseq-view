#[derive(Debug, Clone, PartialEq)]
pub enum TaskState {
    Todo,
    Done,
    Later,
    Now,
    Waiting,
    Cancelled,
}

impl TaskState {
    /// Returns the keyword string for this task state (e.g., "TODO", "DONE").
    pub fn keyword(&self) -> &'static str {
        match self {
            TaskState::Todo => "TODO",
            TaskState::Done => "DONE",
            TaskState::Later => "LATER",
            TaskState::Now => "NOW",
            TaskState::Waiting => "WAITING",
            TaskState::Cancelled => "CANCELLED",
        }
    }
}

/// A single logical line of parsed content.
///
/// `src_start`/`src_end` record the RAW-file line range this `ParsedLine`
/// was parsed from, as a 0-based, HALF-OPEN range `[src_start, src_end)`
/// (same convention as `app::block_range_at`). For an ordinary line this is
/// exactly one raw line (`src_end == src_start + 1`). A fenced code block is
/// FOLDED into a single `ParsedLine` (see `parse_file`), so its span covers
/// every raw line from the opening ` ``` ` fence through the closing fence
/// (or through EOF, if unterminated) — `content_lines` index therefore does
/// NOT correspond 1:1 with raw file line numbers, which is exactly what
/// these fields exist to bridge.
#[derive(Debug, Clone, Default)]
pub struct ParsedLine {
    pub indent: usize,
    pub is_bullet: bool,
    pub task: Option<TaskState>,
    pub segments: Vec<Segment>,
    pub src_start: usize,
    pub src_end: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Segment {
    Plain(String),
    PageLink(String),
    Tag(String),
    Bold(String),
    Italic(String),
    Code(String),
    BlockRef(String),
    Property(String, String),
}

fn compute_indent(leading_ws: &str) -> usize {
    let mut indent = 0;
    let mut space_count = 0;

    for c in leading_ws.chars() {
        match c {
            '\t' => {
                indent += 1 + space_count / 2;
                space_count = 0;
            }
            ' ' => {
                space_count += 1;
            }
            _ => break,
        }
    }
    // Add remaining spaces (every 2 = 1 level)
    indent += space_count / 2;
    indent
}

/// Strips a fenced code block's own outline-nesting prefix from one of its
/// captured raw lines, and replaces any remaining tab with a single space.
///
/// Logseq indents every continuation line of a block to match that block's
/// own nesting depth (with tab characters), so a folded code block's raw
/// lines carry that outline indentation glued onto the code's own -- e.g.
/// `"\t\t\t    \"category\": ..."` is 3 tabs of outline nesting plus 4
/// spaces of the JSON's own indentation. Left in, those raw tabs flow
/// straight through `wrap::wrap_row_ranges` (zero display width, not a
/// forced break) into a rendered `Span`, and `ratatui::widgets::Paragraph`
/// writes `StyledGrapheme`s straight into buffer cells without the
/// control-character filtering `Buffer::set_stringn` has -- so a literal
/// tab byte reaches the real terminal, which jumps to the next tab stop
/// instead of advancing one column, desyncing the terminal's actual cursor
/// from what the app assumes and corrupting the display well past the
/// pane's own edge.
fn normalize_code_line(raw: &str, block_leading_ws: &str) -> String {
    let stripped = raw.strip_prefix(block_leading_ws).unwrap_or(raw);
    stripped.replace('\t', " ")
}

pub fn parse_file(content: &str) -> Vec<ParsedLine> {
    let mut lines = Vec::new();
    let mut in_code_block = false;
    let mut code_lang = String::new();
    let mut code_buf: Vec<String> = Vec::new();
    // Raw line index (0-based) where the currently-open code block's opening
    // fence was seen; combined with the current `raw_idx` this gives the
    // folded block's `[src_start, src_end)` span.
    let mut code_block_start: usize = 0;
    // The opening fence line's own indent/bullet-ness, preserved onto the
    // folded `ParsedLine` so a code block that IS a Logseq block in its own
    // right (`- ```lang`, the normal way to write one) keeps its place in
    // the outline instead of always rendering as a top-level, non-bullet
    // line regardless of how deeply it was actually nested.
    let mut code_block_indent: usize = 0;
    let mut code_block_is_bullet: bool = false;
    // The opening fence line's own leading whitespace, stripped from every
    // content line as it's captured (see `normalize_code_line`) -- Logseq
    // indents a block's continuation lines to match its own outline
    // nesting (with tabs), which is layout, not part of the code.
    let mut code_block_leading_ws = String::new();

    for (raw_idx, raw) in content.lines().enumerate() {
        if in_code_block {
            if raw.trim_start().starts_with("```") {
                let code = code_buf.join("\n");
                lines.push(ParsedLine {
                    indent: code_block_indent,
                    is_bullet: code_block_is_bullet,
                    task: None,
                    segments: vec![Segment::Code(format!("```{}\n{}\n```", code_lang, code))],
                    src_start: code_block_start,
                    src_end: raw_idx + 1,
                });
                code_buf.clear();
                in_code_block = false;
            } else {
                code_buf.push(normalize_code_line(raw, &code_block_leading_ws));
            }
            continue;
        }

        let trimmed = raw.trim_start();
        let leading = raw.len() - trimmed.len();
        let leading_ws = &raw[..leading];
        let indent = compute_indent(leading_ws);

        let (is_bullet, rest) = if let Some(r) = trimmed.strip_prefix("- ") {
            (true, r)
        } else if trimmed == "-" {
            (true, "")
        } else {
            (false, trimmed)
        };

        // A fenced code block is itself a Logseq block, so its opening
        // fence is normally written as "- ```lang" -- checking the
        // bullet-stripped `rest` (rather than `trimmed`) here means a line
        // like that is still recognized as an opener. Left undetected, the
        // block's own opening never sets `in_code_block`, so a later,
        // unrelated bare "```" line gets misread as opening a fold that
        // then swallows everything up to the next bare fence -- merging
        // unrelated sibling blocks into one garbled `Segment::Code`.
        if rest.starts_with("```") {
            code_lang = rest.trim_start_matches('`').to_string();
            in_code_block = true;
            code_block_start = raw_idx;
            code_block_indent = indent;
            code_block_is_bullet = is_bullet;
            code_block_leading_ws = leading_ws.to_string();
            continue;
        }

        if trimmed.is_empty() {
            lines.push(ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![],
                src_start: raw_idx,
                src_end: raw_idx + 1,
            });
            continue;
        }

        let (task, rest) = extract_task_state(rest);

        // Check property: "key:: value"
        if let Some((key, val)) = rest.split_once(":: ") {
            if !key.contains(' ') {
                lines.push(ParsedLine {
                    indent,
                    is_bullet,
                    task,
                    segments: vec![Segment::Property(key.to_string(), val.to_string())],
                    src_start: raw_idx,
                    src_end: raw_idx + 1,
                });
                continue;
            }
        }

        lines.push(ParsedLine {
            indent,
            is_bullet,
            task,
            segments: parse_inline(rest),
            src_start: raw_idx,
            src_end: raw_idx + 1,
        });
    }

    // Handle unterminated code block at EOF: the fold spans the opening
    // fence line plus every buffered content line (there is no closing
    // fence to include).
    if in_code_block && !code_buf.is_empty() {
        let code = code_buf.join("\n");
        let src_end = code_block_start + 1 + code_buf.len();
        lines.push(ParsedLine {
            indent: code_block_indent,
            is_bullet: code_block_is_bullet,
            task: None,
            segments: vec![Segment::Code(format!("```{}\n{}\n```", code_lang, code))],
            src_start: code_block_start,
            src_end,
        });
    }

    lines
}

/// Replaces the raw lines in the half-open range `[raw_start, raw_end)` of
/// `original` (same convention as `ParsedLine::src_start`/`src_end`) with
/// `replacement`, returning the new full file content. Pure and
/// framework-free — no fs access.
///
/// Normalization rules:
/// - `replacement`'s own lines are read via `str::lines()`, which already
///   ignores a single trailing newline (if present) and strips any `\r`
///   before `\n` — so callers may pass editor output with or without a
///   final newline, and with LF or CRLF endings, without producing a stray
///   blank line or gluing the replacement to the following line.
/// - The line-ending style of the OUTPUT (`\n` vs `\r\n`) always matches
///   `original`'s, regardless of `replacement`'s own line endings.
/// - The OUTPUT's trailing-newline state (whether it ends with a newline or
///   not) always matches `original`'s trailing-newline state, regardless of
///   `replacement` or of which lines were replaced (including the last
///   line/EOF).
/// - `raw_start == raw_end` inserts `replacement` at that position without
///   removing any original line. Out-of-range indices are clamped rather
///   than panicking.
pub fn splice_raw_lines(
    original: &str,
    raw_start: usize,
    raw_end: usize,
    replacement: &str,
) -> String {
    let crlf = original.contains("\r\n");
    let trailing_newline = original.ends_with('\n');
    let sep = if crlf { "\r\n" } else { "\n" };

    let original_lines: Vec<&str> = original.lines().collect();
    let raw_start = raw_start.min(original_lines.len());
    let raw_end = raw_end.clamp(raw_start, original_lines.len());

    let replacement_lines: Vec<&str> = replacement.lines().collect();

    let mut new_lines: Vec<&str> =
        Vec::with_capacity(raw_start + replacement_lines.len() + (original_lines.len() - raw_end));
    new_lines.extend_from_slice(&original_lines[..raw_start]);
    new_lines.extend_from_slice(&replacement_lines);
    new_lines.extend_from_slice(&original_lines[raw_end..]);

    let mut result = new_lines.join(sep);
    if trailing_newline {
        result.push_str(sep);
    }
    result
}

/// Extract plain text from a ParsedLine's segments for search purposes.
/// Concatenates the textual content of each segment, ignoring formatting.
/// If the line has a task state, the keyword (e.g., "TODO", "DONE") is prepended.
pub fn line_to_plain_text(line: &ParsedLine) -> String {
    let mut text = String::new();

    // Include task keyword if present
    if let Some(ref task) = line.task {
        text.push_str(task.keyword());
        text.push(' ');
    }

    for segment in &line.segments {
        match segment {
            Segment::Plain(s) => text.push_str(s),
            Segment::PageLink(s) => text.push_str(s),
            Segment::Tag(s) => {
                text.push('#');
                text.push_str(s);
            }
            Segment::Bold(s) => text.push_str(s),
            Segment::Italic(s) => text.push_str(s),
            Segment::Code(s) => text.push_str(s),
            Segment::BlockRef(s) => text.push_str(s),
            Segment::Property(key, val) => {
                text.push_str(key);
                text.push_str(":: ");
                text.push_str(val);
            }
        }
    }
    text
}

/// What kind of thing a `DisplayFragment` is, e.g. for `ui::render_line` to
/// pick a style. Purely a label -- carries no styling itself, so this stays
/// framework-agnostic.
#[derive(Debug, Clone, PartialEq)]
pub enum FragmentKind {
    Indent,
    Bullet,
    TaskLabel(TaskState),
    Plain,
    PageLink,
    Tag,
    Bold,
    Italic,
    Code,
    BlockRef,
    PropertyKey,
    PropertySeparator,
    PropertyValue,
}

/// One labeled piece of a `ParsedLine`'s displayed text, in render order.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayFragment {
    pub text: String,
    pub kind: FragmentKind,
}

/// Breaks a `ParsedLine` down into the exact sequence of text fragments it
/// displays as -- indent, bullet marker, task-state label, and segment
/// decorations (`[[page links]]`, `#tags`, `((block refs))`, `key:: val`
/// properties). This is the SINGLE place that text exists: `ui::render_line`
/// consumes these fragments and attaches a style to each `kind` rather than
/// reconstructing the text itself, and `line_display_text` (used by
/// `line_row_count` for wrap-row math) just concatenates them. Because both
/// read from the same fragments, they cannot drift apart the way two
/// independent text-building implementations could -- which is exactly the
/// failure mode that let #71's "lines pushed off screen" bug happen in the
/// first place (the scroll model and the renderer silently disagreeing about
/// what a line's rows actually contain).
pub fn line_display_fragments(line: &ParsedLine) -> Vec<DisplayFragment> {
    let mut fragments = Vec::new();

    if line.indent > 0 {
        fragments.push(DisplayFragment {
            text: "  ".repeat(line.indent),
            kind: FragmentKind::Indent,
        });
    }

    if line.is_bullet {
        let bullet = match line.task {
            Some(TaskState::Done) | Some(TaskState::Cancelled) => "✓ ",
            Some(_) => "○ ",
            None => "• ",
        };
        fragments.push(DisplayFragment {
            text: bullet.to_string(),
            kind: FragmentKind::Bullet,
        });
    }

    if let Some(ref task) = line.task {
        fragments.push(DisplayFragment {
            text: format!("{} ", task.keyword()),
            kind: FragmentKind::TaskLabel(task.clone()),
        });
    }

    for segment in &line.segments {
        match segment {
            Segment::Plain(s) => fragments.push(DisplayFragment {
                text: s.clone(),
                kind: FragmentKind::Plain,
            }),
            Segment::PageLink(s) => fragments.push(DisplayFragment {
                text: format!("[[{}]]", s),
                kind: FragmentKind::PageLink,
            }),
            Segment::Tag(s) => fragments.push(DisplayFragment {
                text: format!("#{}", s),
                kind: FragmentKind::Tag,
            }),
            Segment::Bold(s) => fragments.push(DisplayFragment {
                text: s.clone(),
                kind: FragmentKind::Bold,
            }),
            Segment::Italic(s) => fragments.push(DisplayFragment {
                text: s.clone(),
                kind: FragmentKind::Italic,
            }),
            Segment::Code(s) => fragments.push(DisplayFragment {
                text: s.clone(),
                kind: FragmentKind::Code,
            }),
            Segment::BlockRef(s) => {
                let preview: String = s.chars().take(8).collect();
                fragments.push(DisplayFragment {
                    text: format!("(({}…))", preview),
                    kind: FragmentKind::BlockRef,
                });
            }
            Segment::Property(key, val) => {
                fragments.push(DisplayFragment {
                    text: key.clone(),
                    kind: FragmentKind::PropertyKey,
                });
                fragments.push(DisplayFragment {
                    text: ":: ".to_string(),
                    kind: FragmentKind::PropertySeparator,
                });
                fragments.push(DisplayFragment {
                    text: val.clone(),
                    kind: FragmentKind::PropertyValue,
                });
            }
        }
    }

    fragments
}

/// The full text of a `ParsedLine` exactly as it's displayed (see
/// `line_display_fragments`), which is what `line_row_count` needs to
/// measure the width `ui::render_line`'s output actually occupies.
pub fn line_display_text(line: &ParsedLine) -> String {
    line_display_fragments(line)
        .iter()
        .map(|f| f.text.as_str())
        .collect()
}

/// The number of wrapped terminal rows `line` needs at `width` columns.
/// Centralizes `line_display_text` + `wrap::row_count` so every caller
/// (scroll clamp, windowing, scrollbar) measures rows the same way.
pub fn line_row_count(line: &ParsedLine, width: usize) -> usize {
    crate::wrap::row_count(&line_display_text(line), width)
}

fn extract_task_state(s: &str) -> (Option<TaskState>, &str) {
    const STATES: &[(&str, TaskState)] = &[
        ("TODO ", TaskState::Todo),
        ("DONE ", TaskState::Done),
        ("LATER ", TaskState::Later),
        ("NOW ", TaskState::Now),
        ("WAITING ", TaskState::Waiting),
        ("CANCELLED ", TaskState::Cancelled),
    ];
    for (prefix, state) in STATES {
        if let Some(rest) = s.strip_prefix(prefix) {
            return (Some(state.clone()), rest);
        }
    }
    (None, s)
}

fn parse_inline(s: &str) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut buf = String::new();

    macro_rules! flush {
        () => {
            if !buf.is_empty() {
                segments.push(Segment::Plain(buf.clone()));
                buf.clear();
            }
        };
    }

    let s_bytes = s.as_bytes();
    let mut i = 0;

    while i < s.len() {
        // [[page link]]
        if s[i..].starts_with("[[") {
            if let Some(end) = s[i + 2..].find("]]") {
                flush!();
                let link = s[i + 2..i + 2 + end].to_string();
                segments.push(Segment::PageLink(link));
                i += 2 + end + 2;
                continue;
            }
        }

        // ((block-ref))
        if s[i..].starts_with("((") {
            if let Some(end) = s[i + 2..].find("))") {
                flush!();
                let refid = s[i + 2..i + 2 + end].to_string();
                segments.push(Segment::BlockRef(refid));
                i += 2 + end + 2;
                continue;
            }
        }

        // **bold**
        if s[i..].starts_with("**") {
            if let Some(end) = s[i + 2..].find("**") {
                flush!();
                let text = s[i + 2..i + 2 + end].to_string();
                segments.push(Segment::Bold(text));
                i += 2 + end + 2;
                continue;
            }
        }

        // `code`
        if s_bytes.get(i) == Some(&b'`') {
            if let Some(end) = s[i + 1..].find('`') {
                flush!();
                let code = s[i + 1..i + 1 + end].to_string();
                segments.push(Segment::Code(code));
                i += 1 + end + 1;
                continue;
            }
        }

        // *italic* or _italic_ (single, not double)
        if s_bytes.get(i) == Some(&b'*') && s_bytes.get(i + 1) != Some(&b'*') {
            // *italic* - simple case, no boundary checks needed
            if let Some(end) = s[i + 1..].find('*') {
                flush!();
                let text = s[i + 1..i + 1 + end].to_string();
                segments.push(Segment::Italic(text));
                i += 1 + end + 1;
                continue;
            }
        }

        // _italic_ - requires word boundaries
        if s_bytes.get(i) == Some(&b'_') {
            // Check word boundary before opening _
            let before_is_boundary = i == 0 || {
                let prev_char = s[..i].chars().last().unwrap();
                !prev_char.is_alphanumeric()
            };

            if before_is_boundary {
                if let Some(end) = s[i + 1..].find('_') {
                    // Check word boundary after closing _
                    let after_idx = i + 1 + end + 1;
                    let after_is_boundary = after_idx >= s.len() || {
                        let next_char = s[after_idx..].chars().next().unwrap();
                        !next_char.is_alphanumeric()
                    };

                    if after_is_boundary {
                        flush!();
                        let text = s[i + 1..i + 1 + end].to_string();
                        segments.push(Segment::Italic(text));
                        i = after_idx;
                        continue;
                    }
                }
            }
        }

        // #tag (word boundary after #)
        if s_bytes.get(i) == Some(&b'#') {
            let rest = &s[i + 1..];
            let tag_end = rest
                .find(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
                .unwrap_or(rest.len());
            if tag_end > 0 {
                flush!();
                let tag = rest[..tag_end].to_string();
                segments.push(Segment::Tag(tag));
                i += 1 + tag_end;
                continue;
            }
        }

        // gather remaining char
        let c = s[i..].chars().next().unwrap();
        buf.push(c);
        i += c.len_utf8();
    }

    flush!();
    segments
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============ line_display_text / line_row_count tests ============

    #[test]
    fn line_display_text_includes_indent_bullet_and_task_label() {
        let line = ParsedLine {
            indent: 2,
            is_bullet: true,
            task: Some(TaskState::Todo),
            segments: vec![Segment::Plain("write tests".to_string())],
            ..Default::default()
        };
        assert_eq!(line_display_text(&line), "    ○ TODO write tests");
    }

    #[test]
    fn line_display_text_marks_done_and_cancelled_with_check() {
        let line = ParsedLine {
            indent: 0,
            is_bullet: true,
            task: Some(TaskState::Done),
            segments: vec![Segment::Plain("shipped".to_string())],
            ..Default::default()
        };
        assert_eq!(line_display_text(&line), "✓ DONE shipped");
    }

    #[test]
    fn line_display_text_wraps_segments_like_render_line() {
        let line = ParsedLine {
            indent: 0,
            is_bullet: false,
            task: None,
            segments: vec![
                Segment::Plain("see ".to_string()),
                Segment::PageLink("Other Page".to_string()),
                Segment::Plain(" and ".to_string()),
                Segment::Tag("logseq".to_string()),
                Segment::Plain(" re ".to_string()),
                Segment::BlockRef("0123456789".to_string()),
            ],
            ..Default::default()
        };
        assert_eq!(
            line_display_text(&line),
            "see [[Other Page]] and #logseq re ((01234567…))"
        );
    }

    #[test]
    fn line_display_text_property_matches_key_double_colon_val() {
        let line = ParsedLine {
            indent: 0,
            is_bullet: false,
            task: None,
            segments: vec![Segment::Property(
                "status".to_string(),
                "active".to_string(),
            )],
            ..Default::default()
        };
        assert_eq!(line_display_text(&line), "status:: active");
    }

    #[test]
    fn line_row_count_matches_wrap_row_count_of_display_text() {
        let line = ParsedLine {
            indent: 0,
            is_bullet: false,
            task: None,
            segments: vec![Segment::Plain("x".repeat(200))],
            ..Default::default()
        };
        assert_eq!(
            line_row_count(&line, 19),
            crate::wrap::row_count(&"x".repeat(200), 19)
        );
    }

    #[test]
    fn line_row_count_is_one_for_a_short_line() {
        let line = ParsedLine {
            indent: 0,
            is_bullet: false,
            task: None,
            segments: vec![Segment::Plain("short".to_string())],
            ..Default::default()
        };
        assert_eq!(line_row_count(&line, 80), 1);
    }

    // ============ parse_inline tests ============

    #[test]
    fn test_parse_inline_page_link() {
        let result = parse_inline("Check [[my page]] here");
        assert_eq!(
            result,
            vec![
                Segment::Plain("Check ".to_string()),
                Segment::PageLink("my page".to_string()),
                Segment::Plain(" here".to_string()),
            ]
        );
    }

    #[test]
    fn test_parse_inline_tags() {
        let result = parse_inline("This is #tag and #my-tag and #tag_name");
        assert_eq!(
            result,
            vec![
                Segment::Plain("This is ".to_string()),
                Segment::Tag("tag".to_string()),
                Segment::Plain(" and ".to_string()),
                Segment::Tag("my-tag".to_string()),
                Segment::Plain(" and ".to_string()),
                Segment::Tag("tag_name".to_string()),
            ]
        );
    }

    #[test]
    fn test_parse_inline_tag_with_punctuation() {
        let result = parse_inline("Tagged with #issue, right?");
        assert_eq!(
            result,
            vec![
                Segment::Plain("Tagged with ".to_string()),
                Segment::Tag("issue".to_string()),
                Segment::Plain(", right?".to_string()),
            ]
        );
    }

    #[test]
    fn test_parse_inline_bold() {
        let result = parse_inline("This is **bold** text");
        assert_eq!(
            result,
            vec![
                Segment::Plain("This is ".to_string()),
                Segment::Bold("bold".to_string()),
                Segment::Plain(" text".to_string()),
            ]
        );
    }

    #[test]
    fn test_parse_inline_code() {
        let result = parse_inline("Use `code here` in text");
        assert_eq!(
            result,
            vec![
                Segment::Plain("Use ".to_string()),
                Segment::Code("code here".to_string()),
                Segment::Plain(" in text".to_string()),
            ]
        );
    }

    #[test]
    fn test_parse_inline_italic_asterisk() {
        let result = parse_inline("This is *italic* text");
        assert_eq!(
            result,
            vec![
                Segment::Plain("This is ".to_string()),
                Segment::Italic("italic".to_string()),
                Segment::Plain(" text".to_string()),
            ]
        );
    }

    #[test]
    fn test_parse_inline_italic_underscore() {
        let result = parse_inline("This is _italic_ text");
        assert_eq!(
            result,
            vec![
                Segment::Plain("This is ".to_string()),
                Segment::Italic("italic".to_string()),
                Segment::Plain(" text".to_string()),
            ]
        );
    }

    #[test]
    fn test_parse_inline_italic_underscore_no_false_positives() {
        // Issue #14.1: foo_bar_baz should NOT be parsed as italic
        let result = parse_inline("foo_bar_baz");
        assert_eq!(result, vec![Segment::Plain("foo_bar_baz".to_string())]);
    }

    #[test]
    fn test_parse_inline_italic_underscore_word_boundary() {
        // _text_ should work at word boundaries
        let result = parse_inline("prefix _italic_ suffix");
        assert_eq!(
            result,
            vec![
                Segment::Plain("prefix ".to_string()),
                Segment::Italic("italic".to_string()),
                Segment::Plain(" suffix".to_string()),
            ]
        );
    }

    #[test]
    fn test_parse_inline_italic_underscore_start() {
        // _text_ at start of string
        let result = parse_inline("_italic_ text");
        assert_eq!(
            result,
            vec![
                Segment::Italic("italic".to_string()),
                Segment::Plain(" text".to_string()),
            ]
        );
    }

    #[test]
    fn test_parse_inline_italic_underscore_end() {
        // _text_ at end of string
        let result = parse_inline("text _italic_");
        assert_eq!(
            result,
            vec![
                Segment::Plain("text ".to_string()),
                Segment::Italic("italic".to_string()),
            ]
        );
    }

    #[test]
    fn test_parse_inline_italic_underscore_after_punctuation() {
        // _text_ after punctuation should work
        let result = parse_inline("text, _italic_ here");
        assert_eq!(
            result,
            vec![
                Segment::Plain("text, ".to_string()),
                Segment::Italic("italic".to_string()),
                Segment::Plain(" here".to_string()),
            ]
        );
    }

    #[test]
    fn test_parse_inline_block_ref() {
        let result = parse_inline("See ((block-id-123)) for details");
        assert_eq!(
            result,
            vec![
                Segment::Plain("See ".to_string()),
                Segment::BlockRef("block-id-123".to_string()),
                Segment::Plain(" for details".to_string()),
            ]
        );
    }

    #[test]
    fn test_parse_inline_mixed() {
        let result = parse_inline("foo [[bar]] **baz** *italic* #tag");
        assert_eq!(
            result,
            vec![
                Segment::Plain("foo ".to_string()),
                Segment::PageLink("bar".to_string()),
                Segment::Plain(" ".to_string()),
                Segment::Bold("baz".to_string()),
                Segment::Plain(" ".to_string()),
                Segment::Italic("italic".to_string()),
                Segment::Plain(" ".to_string()),
                Segment::Tag("tag".to_string()),
            ]
        );
    }

    // ============ extract_task_state tests ============

    #[test]
    fn test_extract_task_state_todo() {
        let (state, rest) = extract_task_state("TODO do something");
        assert_eq!(state, Some(TaskState::Todo));
        assert_eq!(rest, "do something");
    }

    #[test]
    fn test_extract_task_state_done() {
        let (state, rest) = extract_task_state("DONE finished");
        assert_eq!(state, Some(TaskState::Done));
        assert_eq!(rest, "finished");
    }

    #[test]
    fn test_extract_task_state_later() {
        let (state, rest) = extract_task_state("LATER defer this");
        assert_eq!(state, Some(TaskState::Later));
        assert_eq!(rest, "defer this");
    }

    #[test]
    fn test_extract_task_state_now() {
        let (state, rest) = extract_task_state("NOW urgent task");
        assert_eq!(state, Some(TaskState::Now));
        assert_eq!(rest, "urgent task");
    }

    #[test]
    fn test_extract_task_state_waiting() {
        let (state, rest) = extract_task_state("WAITING on someone");
        assert_eq!(state, Some(TaskState::Waiting));
        assert_eq!(rest, "on someone");
    }

    #[test]
    fn test_extract_task_state_cancelled() {
        let (state, rest) = extract_task_state("CANCELLED cancelled task");
        assert_eq!(state, Some(TaskState::Cancelled));
        assert_eq!(rest, "cancelled task");
    }

    #[test]
    fn test_extract_task_state_none() {
        let (state, rest) = extract_task_state("Just regular text");
        assert_eq!(state, None);
        assert_eq!(rest, "Just regular text");
    }

    // ============ parse_file tests ============

    #[test]
    fn test_parse_file_simple_line() {
        let content = "Hello world";
        let result = parse_file(content);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].indent, 0);
        assert!(!result[0].is_bullet);
        assert_eq!(result[0].task, None);
        assert_eq!(
            result[0].segments,
            vec![Segment::Plain("Hello world".to_string())]
        );
    }

    #[test]
    fn test_parse_file_two_space_indent() {
        let content = "  Indented";
        let result = parse_file(content);
        assert_eq!(result[0].indent, 1);
    }

    #[test]
    fn test_parse_file_four_space_indent() {
        let content = "    Indented";
        let result = parse_file(content);
        assert_eq!(result[0].indent, 2);
    }

    #[test]
    fn test_parse_file_tab_indent() {
        let content = "\tIndented";
        let result = parse_file(content);
        assert_eq!(result[0].indent, 1);
    }

    #[test]
    fn test_parse_file_two_tabs_indent() {
        let content = "\t\tIndented";
        let result = parse_file(content);
        assert_eq!(result[0].indent, 2);
    }

    #[test]
    fn test_parse_file_mixed_tab_and_spaces_indent() {
        let content = "\t  Mixed";
        let result = parse_file(content);
        // 1 tab (1 level) + 2 spaces (1 level) = 2 levels
        assert_eq!(result[0].indent, 2);
    }

    #[test]
    fn test_parse_file_bullet() {
        let content = "- Item one";
        let result = parse_file(content);
        assert!(result[0].is_bullet);
        assert_eq!(
            result[0].segments,
            vec![Segment::Plain("Item one".to_string())]
        );
    }

    #[test]
    fn test_parse_file_bullet_with_task() {
        let content = "- TODO item";
        let result = parse_file(content);
        assert!(result[0].is_bullet);
        assert_eq!(result[0].task, Some(TaskState::Todo));
        assert_eq!(result[0].segments, vec![Segment::Plain("item".to_string())]);
    }

    #[test]
    fn test_parse_file_property() {
        let content = "key:: value";
        let result = parse_file(content);
        assert_eq!(
            result[0].segments,
            vec![Segment::Property("key".to_string(), "value".to_string())]
        );
    }

    #[test]
    fn test_parse_file_blank_line() {
        let content = "Line one\n\nLine three";
        let result = parse_file(content);
        assert_eq!(result.len(), 3);
        assert_eq!(result[1].segments.len(), 0); // blank line
    }

    #[test]
    fn test_parse_file_fenced_code_block() {
        let content = "```rust\nfn main() {\n    println!(\"Hello\");\n}\n```";
        let result = parse_file(content);
        assert_eq!(result.len(), 1);
        match &result[0].segments[0] {
            Segment::Code(code) => {
                assert!(code.contains("fn main"));
            }
            _ => panic!("Expected Code segment"),
        }
    }

    #[test]
    fn test_parse_file_code_block_strips_tab_outline_indentation() {
        // Regression for the corruption reported in the wild: Logseq
        // indents a block's continuation lines with tabs to match its own
        // outline nesting -- e.g. "\t\t\t  Extract the following fields"
        // under a block opened by "\t\t\t- ```". That tab prefix is layout,
        // not code, and previously survived verbatim into `Segment::Code`.
        // A raw tab has zero display width in `wrap::wrap_row_ranges`, so
        // it never forced a wrap, and `ratatui::widgets::Paragraph` writes
        // `StyledGrapheme`s straight into buffer cells with no
        // control-character filtering -- so the byte reached the real
        // terminal, which jumps to the next tab stop instead of advancing
        // one column, corrupting the display well past the pane's edge.
        let content = "\t\t\t- ```\n\t\t\t  fn main() {}\n\t\t\t  ```";
        let result = parse_file(content);
        assert_eq!(result.len(), 1);
        match &result[0].segments[0] {
            Segment::Code(code) => {
                assert!(
                    !code.contains('\t'),
                    "no raw tab byte should survive into the folded code text: {code:?}"
                );
                assert!(code.contains("fn main() {}"));
            }
            _ => panic!("Expected Code segment"),
        }
    }

    #[test]
    fn test_parse_file_code_block_preserves_own_indentation_past_the_outline_prefix() {
        // Stripping the block's own outline-nesting tabs must not touch the
        // code's OWN indentation, written as plain spaces after that prefix
        // (e.g. nested JSON) -- only the shared tab prefix goes.
        let content = "\t\t- ```json\n\t\t  {\n\t\t    \"a\": 1\n\t\t  }\n\t\t  ```";
        let result = parse_file(content);
        match &result[0].segments[0] {
            Segment::Code(code) => {
                assert!(code.contains("  {"));
                assert!(code.contains("    \"a\": 1"));
            }
            _ => panic!("Expected Code segment"),
        }
    }

    #[test]
    fn test_parse_file_code_block_opened_by_a_bullet() {
        // A fenced code block is itself a Logseq block, so it's normally
        // written as "- ```lang" -- the bullet marker and the fence on the
        // same line. Before this was recognized as an opener, the fence
        // check only looked at the raw line (still starting with "- ", not
        // "```"), so the block's own opening was silently skipped, and
        // whatever unrelated bare "```" line appeared later in the file
        // was misread as opening the fold instead -- merging unrelated
        // sibling blocks into one garbled code segment (see the sibling
        // test below).
        let content = "- ```rust\n  fn main() {}\n  ```";
        let result = parse_file(content);
        assert_eq!(result.len(), 1);
        assert!(result[0].is_bullet);
        match &result[0].segments[0] {
            Segment::Code(code) => assert!(code.contains("fn main")),
            _ => panic!("Expected Code segment"),
        }
    }

    #[test]
    fn test_parse_file_code_block_opened_by_a_bullet_preserves_indent() {
        // The folded ParsedLine must keep the opening bullet's own nesting
        // depth, not always render as a top-level line regardless of how
        // deeply the code block was actually nested under its parent.
        let content = "- parent\n  - ```rust\n    fn main() {}\n    ```";
        let result = parse_file(content);
        assert_eq!(result.len(), 2);
        assert_eq!(result[1].indent, 1);
        assert!(result[1].is_bullet);
    }

    #[test]
    fn test_parse_file_unrelated_bullets_after_a_bullet_opened_code_block_are_not_swallowed() {
        // Regression for the exact corruption reported in the wild: a
        // sibling bullet ("- v2") and its own bullet-opened code block
        // following one already-closed, bullet-opened code block must stay
        // separate blocks -- not get folded together into one giant code
        // segment because the first block's opener went unrecognized.
        let content =
            "- v1\n  - ```\n    line one\n    ```\n- v2\n- ```markdown\n  prompt text\n  ```";
        let result = parse_file(content);

        let plain_texts: Vec<&str> = result
            .iter()
            .filter_map(|l| match l.segments.first() {
                Some(Segment::Plain(s)) => Some(s.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            plain_texts,
            vec!["v1", "v2"],
            "\"v2\" must remain its own bullet, not get absorbed into a code block"
        );

        let code_blocks: Vec<&str> = result
            .iter()
            .filter_map(|l| match l.segments.first() {
                Some(Segment::Code(s)) => Some(s.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(code_blocks.len(), 2, "each code block stays its own fold");
        assert!(code_blocks[0].contains("line one"));
        assert!(!code_blocks[0].contains("v2"));
        assert!(code_blocks[1].contains("prompt text"));
    }

    #[test]
    fn test_parse_file_unterminated_code_block() {
        // Issue #14.2: unterminated code block should be preserved
        let content = "```rust\nfn main() {\n    println!(\"Hello\");\n}";
        let result = parse_file(content);
        assert_eq!(result.len(), 1);
        // The code block should be captured even without closing ```
        match &result[0].segments[0] {
            Segment::Code(code) => {
                assert!(code.contains("fn main"));
                assert!(code.starts_with("```rust"));
            }
            _ => panic!("Expected Code segment"),
        }
    }

    #[test]
    fn test_parse_file_with_lang() {
        let content = "```python\nprint(\"hello\")\n```";
        let result = parse_file(content);
        assert_eq!(result.len(), 1);
        match &result[0].segments[0] {
            Segment::Code(code) => {
                assert!(code.contains("python"));
            }
            _ => panic!("Expected Code segment"),
        }
    }

    #[test]
    fn test_parse_file_multiple_blocks() {
        let content = "Line one\n\nLine two";
        let result = parse_file(content);
        assert_eq!(result.len(), 3);
        assert_eq!(
            result[0].segments[0],
            Segment::Plain("Line one".to_string())
        );
        assert_eq!(
            result[2].segments[0],
            Segment::Plain("Line two".to_string())
        );
    }

    // ============ line_to_plain_text tests ============

    #[test]
    fn test_line_to_plain_text_plain_only() {
        let line = ParsedLine {
            indent: 0,
            is_bullet: false,
            task: None,
            segments: vec![Segment::Plain("Hello world".to_string())],
            ..Default::default()
        };
        assert_eq!(line_to_plain_text(&line), "Hello world");
    }

    #[test]
    fn test_line_to_plain_text_mixed_segments() {
        let line = ParsedLine {
            indent: 0,
            is_bullet: false,
            task: None,
            segments: vec![
                Segment::Plain("Check ".to_string()),
                Segment::PageLink("my page".to_string()),
                Segment::Plain(" here ".to_string()),
                Segment::Tag("tag".to_string()),
                Segment::Plain(" and ".to_string()),
                Segment::Bold("bold".to_string()),
            ],
            ..Default::default()
        };
        assert_eq!(
            line_to_plain_text(&line),
            "Check my page here #tag and bold"
        );
    }

    #[test]
    fn test_line_to_plain_text_code_segment() {
        let line = ParsedLine {
            indent: 0,
            is_bullet: false,
            task: None,
            segments: vec![
                Segment::Plain("Use ".to_string()),
                Segment::Code("code here".to_string()),
                Segment::Plain(" in text".to_string()),
            ],
            ..Default::default()
        };
        assert_eq!(line_to_plain_text(&line), "Use code here in text");
    }

    #[test]
    fn test_line_to_plain_text_property() {
        let line = ParsedLine {
            indent: 0,
            is_bullet: false,
            task: None,
            segments: vec![Segment::Property("key".to_string(), "value".to_string())],
            ..Default::default()
        };
        assert_eq!(line_to_plain_text(&line), "key:: value");
    }

    #[test]
    fn test_line_to_plain_text_block_ref() {
        let line = ParsedLine {
            indent: 0,
            is_bullet: false,
            task: None,
            segments: vec![
                Segment::Plain("See ".to_string()),
                Segment::BlockRef("block-id-123".to_string()),
                Segment::Plain(" for details".to_string()),
            ],
            ..Default::default()
        };
        assert_eq!(line_to_plain_text(&line), "See block-id-123 for details");
    }

    #[test]
    fn test_line_to_plain_text_empty() {
        let line = ParsedLine {
            indent: 0,
            is_bullet: false,
            task: None,
            segments: vec![],
            ..Default::default()
        };
        assert_eq!(line_to_plain_text(&line), "");
    }

    #[test]
    fn test_line_to_plain_text_italic_and_bold() {
        let line = ParsedLine {
            indent: 0,
            is_bullet: false,
            task: None,
            segments: vec![
                Segment::Italic("italic".to_string()),
                Segment::Plain(" and ".to_string()),
                Segment::Bold("bold".to_string()),
            ],
            ..Default::default()
        };
        assert_eq!(line_to_plain_text(&line), "italic and bold");
    }

    // ============ TaskState::keyword tests ============

    #[test]
    fn test_task_state_keyword() {
        assert_eq!(TaskState::Todo.keyword(), "TODO");
        assert_eq!(TaskState::Done.keyword(), "DONE");
        assert_eq!(TaskState::Later.keyword(), "LATER");
        assert_eq!(TaskState::Now.keyword(), "NOW");
        assert_eq!(TaskState::Waiting.keyword(), "WAITING");
        assert_eq!(TaskState::Cancelled.keyword(), "CANCELLED");
    }

    // ============ line_to_plain_text with task tests ============

    #[test]
    fn test_line_to_plain_text_with_task_todo() {
        let line = ParsedLine {
            indent: 0,
            is_bullet: false,
            task: Some(TaskState::Todo),
            segments: vec![Segment::Plain("buy milk".to_string())],
            ..Default::default()
        };
        assert_eq!(line_to_plain_text(&line), "TODO buy milk");
    }

    #[test]
    fn test_line_to_plain_text_with_task_done() {
        let line = ParsedLine {
            indent: 0,
            is_bullet: false,
            task: Some(TaskState::Done),
            segments: vec![Segment::Plain("finished task".to_string())],
            ..Default::default()
        };
        assert_eq!(line_to_plain_text(&line), "DONE finished task");
    }

    #[test]
    fn test_line_to_plain_text_with_task_cancelled() {
        let line = ParsedLine {
            indent: 0,
            is_bullet: false,
            task: Some(TaskState::Cancelled),
            segments: vec![Segment::Plain("abandoned".to_string())],
            ..Default::default()
        };
        assert_eq!(line_to_plain_text(&line), "CANCELLED abandoned");
    }

    #[test]
    fn test_line_to_plain_text_with_task_and_mixed_segments() {
        let line = ParsedLine {
            indent: 0,
            is_bullet: false,
            task: Some(TaskState::Later),
            segments: vec![
                Segment::Plain("Review ".to_string()),
                Segment::PageLink("documentation".to_string()),
                Segment::Plain(" tomorrow".to_string()),
            ],
            ..Default::default()
        };
        assert_eq!(
            line_to_plain_text(&line),
            "LATER Review documentation tomorrow"
        );
    }

    // ============ ParsedLine source-span tests (parse_file) ============

    #[test]
    fn span_plain_line_is_single_raw_line() {
        let result = parse_file("Hello world");
        assert_eq!(result[0].src_start, 0);
        assert_eq!(result[0].src_end, 1);
    }

    #[test]
    fn span_multiple_plain_lines_track_raw_index() {
        let result = parse_file("first\nsecond\nthird");
        assert_eq!(result.len(), 3);
        assert_eq!((result[0].src_start, result[0].src_end), (0, 1));
        assert_eq!((result[1].src_start, result[1].src_end), (1, 2));
        assert_eq!((result[2].src_start, result[2].src_end), (2, 3));
    }

    #[test]
    fn span_nested_indented_lines() {
        let content = "- A\n  - A1\n    - A1a\n  - A2\n- B";
        let result = parse_file(content);
        assert_eq!(result.len(), 5);
        for (i, line) in result.iter().enumerate() {
            assert_eq!(line.src_start, i);
            assert_eq!(line.src_end, i + 1);
        }
    }

    #[test]
    fn span_blank_line() {
        let result = parse_file("Line one\n\nLine three");
        assert_eq!((result[1].src_start, result[1].src_end), (1, 2));
    }

    #[test]
    fn span_property_line() {
        let result = parse_file("intro\nkey:: value\nmore");
        assert_eq!((result[1].src_start, result[1].src_end), (1, 2));
    }

    #[test]
    fn span_fenced_code_block_covers_all_raw_lines() {
        // Raw lines: 0 "before", 1 "```rust", 2 "fn main() {", 3 "  x();",
        // 4 "}", 5 "```", 6 "after" -- the folded ParsedLine for the code
        // block must span [1, 6) (opening fence through closing fence).
        let content = "before\n```rust\nfn main() {\n  x();\n}\n```\nafter";
        let result = parse_file(content);
        assert_eq!(result.len(), 3); // before, code block, after
        assert_eq!((result[0].src_start, result[0].src_end), (0, 1));
        match &result[1].segments[0] {
            Segment::Code(code) => assert!(code.contains("fn main")),
            _ => panic!("expected Code segment"),
        }
        assert_eq!((result[1].src_start, result[1].src_end), (1, 6));
        assert_eq!((result[2].src_start, result[2].src_end), (6, 7));
    }

    #[test]
    fn span_multiple_code_blocks_each_get_own_span() {
        let content = "```rust\nfn a() {}\n```\ngap\n```python\nprint(1)\nprint(2)\n```";
        let result = parse_file(content);
        // 0: code block 1 (raw 0..3), 1: "gap" (raw 3..4), 2: code block 2 (raw 4..8)
        assert_eq!(result.len(), 3);
        assert_eq!((result[0].src_start, result[0].src_end), (0, 3));
        assert_eq!((result[1].src_start, result[1].src_end), (3, 4));
        assert_eq!((result[2].src_start, result[2].src_end), (4, 8));
        match &result[2].segments[0] {
            Segment::Code(code) => assert!(code.contains("python")),
            _ => panic!("expected Code segment"),
        }
    }

    #[test]
    fn span_unterminated_code_block_spans_to_eof() {
        // Raw lines: 0 "```rust", 1 "fn main() {", 2 "  x();" (no closing fence)
        let content = "```rust\nfn main() {\n  x();";
        let result = parse_file(content);
        assert_eq!(result.len(), 1);
        assert_eq!((result[0].src_start, result[0].src_end), (0, 3));
    }

    // ============ splice_raw_lines tests ============

    #[test]
    fn splice_middle_block_replacement() {
        let original = "L0\nL1\nL2\nL3\nL4\n";
        let result = splice_raw_lines(original, 1, 3, "NEW\n");
        assert_eq!(result, "L0\nNEW\nL3\nL4\n");
    }

    #[test]
    fn splice_first_block() {
        let original = "L0\nL1\nL2\n";
        let result = splice_raw_lines(original, 0, 1, "FIRST\n");
        assert_eq!(result, "FIRST\nL1\nL2\n");
    }

    #[test]
    fn splice_last_block_at_eof() {
        let original = "L0\nL1\nL2\n";
        let result = splice_raw_lines(original, 1, 3, "TAIL\n");
        assert_eq!(result, "L0\nTAIL\n");
    }

    #[test]
    fn splice_preserves_trailing_newline_present() {
        let original = "L0\nL1\nL2\n";
        let result = splice_raw_lines(original, 1, 2, "MID");
        assert!(result.ends_with('\n'));
        assert_eq!(result, "L0\nMID\nL2\n");
    }

    #[test]
    fn splice_preserves_no_trailing_newline() {
        let original = "L0\nL1\nL2"; // no trailing newline
        let result = splice_raw_lines(original, 2, 3, "NEW");
        assert!(!result.ends_with('\n'));
        assert_eq!(result, "L0\nL1\nNEW");
    }

    #[test]
    fn splice_no_trailing_newline_replacing_middle_still_no_trailing() {
        let original = "L0\nL1\nL2";
        let result = splice_raw_lines(original, 1, 2, "MID");
        assert_eq!(result, "L0\nMID\nL2");
    }

    #[test]
    fn splice_crlf_line_endings_preserved() {
        let original = "L0\r\nL1\r\nL2\r\n";
        let result = splice_raw_lines(original, 1, 2, "NEW");
        assert_eq!(result, "L0\r\nNEW\r\nL2\r\n");
        assert!(!result.contains("\n\n")); // no corrupted/bare LF glue
    }

    #[test]
    fn splice_crlf_no_trailing_newline_preserved() {
        let original = "L0\r\nL1\r\nL2";
        let result = splice_raw_lines(original, 0, 1, "NEW");
        assert_eq!(result, "NEW\r\nL1\r\nL2");
    }

    #[test]
    fn splice_replacement_without_trailing_newline() {
        let original = "L0\nL1\nL2\n";
        let result = splice_raw_lines(original, 1, 2, "NEW");
        assert_eq!(result, "L0\nNEW\nL2\n");
    }

    #[test]
    fn splice_replacement_with_trailing_newline() {
        let original = "L0\nL1\nL2\n";
        let result = splice_raw_lines(original, 1, 2, "NEW\n");
        assert_eq!(result, "L0\nNEW\nL2\n");
    }

    #[test]
    fn splice_replacement_multiline() {
        let original = "L0\nL1\nL2\n";
        let result = splice_raw_lines(original, 1, 2, "A\nB\nC\n");
        assert_eq!(result, "L0\nA\nB\nC\nL2\n");
    }

    #[test]
    fn splice_whole_file_single_block() {
        let original = "only line\n";
        let result = splice_raw_lines(original, 0, 1, "replaced\n");
        assert_eq!(result, "replaced\n");
    }

    #[test]
    fn splice_out_of_range_indices_clamped_not_panicking() {
        let original = "L0\nL1\n";
        let result = splice_raw_lines(original, 5, 10, "NEW\n");
        // Clamped to EOF: appended at the end, nothing removed.
        assert_eq!(result, "L0\nL1\nNEW\n");
    }
}
