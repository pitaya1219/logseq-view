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

pub fn parse_file(content: &str) -> Vec<ParsedLine> {
    let mut lines = Vec::new();
    let mut in_code_block = false;
    let mut code_lang = String::new();
    let mut code_buf: Vec<String> = Vec::new();
    // Raw line index (0-based) where the currently-open code block's opening
    // fence was seen; combined with the current `raw_idx` this gives the
    // folded block's `[src_start, src_end)` span.
    let mut code_block_start: usize = 0;

    for (raw_idx, raw) in content.lines().enumerate() {
        if in_code_block {
            if raw.trim_start().starts_with("```") {
                let code = code_buf.join("\n");
                lines.push(ParsedLine {
                    indent: 0,
                    is_bullet: false,
                    task: None,
                    segments: vec![Segment::Code(format!("```{}\n{}\n```", code_lang, code))],
                    src_start: code_block_start,
                    src_end: raw_idx + 1,
                });
                code_buf.clear();
                in_code_block = false;
            } else {
                code_buf.push(raw.to_string());
            }
            continue;
        }

        let trimmed = raw.trim_start();

        if trimmed.starts_with("```") {
            code_lang = trimmed.trim_start_matches('`').to_string();
            in_code_block = true;
            code_block_start = raw_idx;
            continue;
        }

        let leading = raw.len() - trimmed.len();
        let leading_ws = &raw[..leading];
        let indent = compute_indent(leading_ws);

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

        let (is_bullet, rest) = if let Some(r) = trimmed.strip_prefix("- ") {
            (true, r)
        } else if trimmed == "-" {
            (true, "")
        } else {
            (false, trimmed)
        };

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
            indent: 0,
            is_bullet: false,
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
