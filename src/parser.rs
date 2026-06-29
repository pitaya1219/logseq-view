#[derive(Debug, Clone, PartialEq)]
pub enum TaskState {
    Todo,
    Done,
    Later,
    Now,
    Waiting,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct ParsedLine {
    pub indent: usize,
    pub is_bullet: bool,
    pub task: Option<TaskState>,
    pub segments: Vec<Segment>,
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

    for raw in content.lines() {
        if in_code_block {
            if raw.trim_start().starts_with("```") {
                let code = code_buf.join("\n");
                lines.push(ParsedLine {
                    indent: 0,
                    is_bullet: false,
                    task: None,
                    segments: vec![Segment::Code(format!("```{}\n{}\n```", code_lang, code))],
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
                });
                continue;
            }
        }

        lines.push(ParsedLine {
            indent,
            is_bullet,
            task,
            segments: parse_inline(rest),
        });
    }

    // Handle unterminated code block at EOF
    if in_code_block && !code_buf.is_empty() {
        let code = code_buf.join("\n");
        lines.push(ParsedLine {
            indent: 0,
            is_bullet: false,
            task: None,
            segments: vec![Segment::Code(format!("```{}\n{}\n```", code_lang, code))],
        });
    }

    lines
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
}
