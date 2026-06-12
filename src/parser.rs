use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

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

#[derive(Debug, Clone)]
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
        let indent = leading / 2;

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
        if (s_bytes.get(i) == Some(&b'*') && s_bytes.get(i + 1) != Some(&b'*'))
            || s_bytes.get(i) == Some(&b'_')
        {
            let delim = s_bytes[i] as char;
            if let Some(end) = s[i + 1..].find(delim) {
                flush!();
                let text = s[i + 1..i + 1 + end].to_string();
                segments.push(Segment::Italic(text));
                i += 1 + end + 1;
                continue;
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

pub fn render_line(parsed: &ParsedLine) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();

    // Indentation
    if parsed.indent > 0 {
        spans.push(Span::raw("  ".repeat(parsed.indent)));
    }

    // Bullet
    if parsed.is_bullet {
        let bullet_char = match parsed.task {
            Some(TaskState::Done) | Some(TaskState::Cancelled) => "✓ ",
            Some(_) => "○ ",
            None => "• ",
        };
        let style = Style::default().fg(Color::DarkGray);
        spans.push(Span::styled(bullet_char.to_string(), style));
    }

    // Task state badge
    if let Some(ref state) = parsed.task {
        let (label, style) = match state {
            TaskState::Todo => ("TODO", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            TaskState::Done => ("DONE", Style::default().fg(Color::Green).add_modifier(Modifier::DIM)),
            TaskState::Later => ("LATER", Style::default().fg(Color::Blue)),
            TaskState::Now => ("NOW", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
            TaskState::Waiting => ("WAITING", Style::default().fg(Color::Cyan)),
            TaskState::Cancelled => ("CANCELLED", Style::default().fg(Color::DarkGray).add_modifier(Modifier::CROSSED_OUT)),
        };
        spans.push(Span::styled(label.to_string(), style));
        spans.push(Span::raw(" "));
    }

    for seg in &parsed.segments {
        match seg {
            Segment::Plain(s) => {
                let style = if matches!(parsed.task, Some(TaskState::Done) | Some(TaskState::Cancelled)) {
                    Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM)
                } else {
                    Style::default()
                };
                spans.push(Span::styled(s.clone(), style));
            }
            Segment::PageLink(s) => {
                spans.push(Span::styled(
                    format!("[[{}]]", s),
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::UNDERLINED),
                ));
            }
            Segment::Tag(s) => {
                spans.push(Span::styled(
                    format!("#{}", s),
                    Style::default().fg(Color::Green),
                ));
            }
            Segment::Bold(s) => {
                spans.push(Span::styled(s.clone(), Style::default().add_modifier(Modifier::BOLD)));
            }
            Segment::Italic(s) => {
                spans.push(Span::styled(s.clone(), Style::default().add_modifier(Modifier::ITALIC)));
            }
            Segment::Code(s) => {
                spans.push(Span::styled(
                    s.clone(),
                    Style::default().fg(Color::Yellow).bg(Color::DarkGray),
                ));
            }
            Segment::BlockRef(s) => {
                spans.push(Span::styled(
                    format!("(({}…))", &s[..s.len().min(8)]),
                    Style::default().fg(Color::Magenta),
                ));
            }
            Segment::Property(key, val) => {
                spans.push(Span::styled(
                    key.clone(),
                    Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::styled(":: ", Style::default().fg(Color::DarkGray)));
                spans.push(Span::raw(val.clone()));
            }
        }
    }

    Line::from(spans)
}
