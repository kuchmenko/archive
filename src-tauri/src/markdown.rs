use std::ops::Range;

use pulldown_cmark::{CowStr, Event, Options, Parser, Tag, TagEnd, html};

struct Reference {
    range: Range<usize>,
    id: i64,
    label: String,
}

pub fn render(markdown: &str) -> String {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_FOOTNOTES;
    let ordinary_ranges = ordinary_text_ranges(markdown, options);
    let references = parse_references(markdown)
        .into_iter()
        .filter(|reference| {
            ordinary_ranges.iter().any(|range| {
                reference.range.start >= range.start && reference.range.end <= range.end
            })
        })
        .collect::<Vec<_>>();
    let (source, placeholders) = replace_references(markdown, &references);
    let mut events = Vec::new();

    for event in Parser::new_ext(&source, options) {
        match event {
            Event::Html(value) | Event::InlineHtml(value) => events.push(Event::Text(value)),
            Event::Text(value) => split_placeholders(value, &placeholders, &mut events),
            other => events.push(other),
        }
    }

    let mut output = String::new();
    html::push_html(&mut output, events.into_iter());
    output
}

fn ordinary_text_ranges(markdown: &str, options: Options) -> Vec<Range<usize>> {
    let mut code_depth = 0;
    let mut html_depth = 0;
    let mut ranges: Vec<Range<usize>> = Vec::new();

    for (event, range) in Parser::new_ext(markdown, options).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(_)) => code_depth += 1,
            Event::End(TagEnd::CodeBlock) => code_depth -= 1,
            Event::Start(Tag::HtmlBlock) => html_depth += 1,
            Event::End(TagEnd::HtmlBlock) => html_depth -= 1,
            Event::Text(_) if code_depth == 0 && html_depth == 0 => {
                if let Some(previous) = ranges.last_mut().filter(|previous| {
                    previous.end == range.start
                        || markdown[previous.end..range.start]
                            .bytes()
                            .all(|byte| byte == b'\\')
                }) {
                    previous.end = range.end;
                } else {
                    ranges.push(range);
                }
            }
            Event::Html(_) | Event::InlineHtml(_) | Event::Code(_) => {}
            _ => {}
        }
    }
    ranges
}

fn parse_references(source: &str) -> Vec<Reference> {
    let bytes = source.as_bytes();
    let mut references = Vec::new();
    let mut start = 0;

    while start + 8 < bytes.len() {
        if !source[start..].starts_with("[[note:") {
            start += source[start..].chars().next().map_or(1, char::len_utf8);
            continue;
        }
        let mut cursor = start + 7;
        let id_start = cursor;
        while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
            cursor += 1;
        }
        if cursor == id_start || bytes.get(cursor) != Some(&b'|') {
            start += 1;
            continue;
        }
        let Ok(id) = source[id_start..cursor].parse::<i64>() else {
            start += 1;
            continue;
        };
        if id <= 0 {
            start += 1;
            continue;
        }
        cursor += 1;
        let mut label = String::new();
        let mut closed = false;
        while cursor < bytes.len() {
            let character = source[cursor..].chars().next().unwrap();
            if matches!(character, '\n' | '\r' | '|') {
                break;
            }
            if character == '\\' {
                let escaped_start = cursor + 1;
                let Some(escaped) = source
                    .get(escaped_start..)
                    .and_then(|rest| rest.chars().next())
                else {
                    break;
                };
                if !matches!(escaped, '\\' | '|' | ']') {
                    break;
                }
                label.push(escaped);
                cursor = escaped_start + escaped.len_utf8();
                continue;
            }
            if character == ']' && bytes.get(cursor + 1) == Some(&b']') {
                references.push(Reference {
                    range: start..cursor + 2,
                    id,
                    label,
                });
                start = cursor + 2;
                closed = true;
                break;
            }
            label.push(character);
            cursor += character.len_utf8();
        }
        if !closed {
            start += 1;
        }
    }
    references
}

fn replace_references(markdown: &str, references: &[Reference]) -> (String, Vec<(String, String)>) {
    let mut prefix = "ARCHIVEINTERNALREFERENCE".to_owned();
    while markdown.contains(&prefix) {
        prefix.push('X');
    }
    let mut source = markdown.to_owned();
    let mut placeholders = Vec::new();
    for (index, reference) in references.iter().enumerate().rev() {
        let token = format!("{prefix}{index}TOKEN");
        let button = format!(
            "<button type=\"button\" data-document-id=\"{}\">{}</button>",
            reference.id,
            escape_html(&reference.label)
        );
        source.replace_range(reference.range.clone(), &token);
        placeholders.push((token, button));
    }
    (source, placeholders)
}

fn split_placeholders<'a>(
    value: CowStr<'a>,
    placeholders: &[(String, String)],
    events: &mut Vec<Event<'a>>,
) {
    let mut parts = vec![(value.into_string(), false)];
    for (token, button) in placeholders {
        let mut next = Vec::new();
        for (part, synthetic) in parts {
            if synthetic {
                next.push((part, true));
                continue;
            }
            let mut remainder = part.as_str();
            while let Some(index) = remainder.find(token) {
                if index > 0 {
                    next.push((remainder[..index].to_owned(), false));
                }
                next.push((button.clone(), true));
                remainder = &remainder[index + token.len()..];
            }
            if !remainder.is_empty() {
                next.push((remainder.to_owned(), false));
            }
        }
        parts = next;
    }
    events.extend(parts.into_iter().map(|(part, synthetic)| {
        let value = CowStr::Boxed(part.into_boxed_str());
        if synthetic {
            Event::Html(value)
        } else {
            Event::Text(value)
        }
    }));
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::render;

    #[test]
    fn escapes_html_and_renders_references() {
        let html = render(
            "# Hi\n\n<script>x</script>\n\nBefore [[note:42|A \\| B]] after `[[note:2|code]]`",
        );
        assert!(html.contains("&lt;script&gt;x&lt;/script&gt;"));
        assert!(
            html.contains(
                "Before <button type=\"button\" data-document-id=\"42\">A | B</button> after"
            ),
            "{html}"
        );
        assert!(html.contains("<code>[[note:2|code]]</code>"));
    }

    #[test]
    fn renders_commonmark_extensions() {
        let html =
            render("~~gone~~\n\n- [x] done\n\n| A |\n| - |\n| B |\n\nword[^1]\n\n[^1]: note");
        assert!(html.contains("<del>gone</del>"));
        assert!(html.contains("type=\"checkbox\""));
        assert!(html.contains("<table>"));
        assert!(html.contains("footnote-reference"));
    }

    #[test]
    fn handles_exact_reference_grammar_and_overflow() {
        let html =
            render(r"[[note:7|a\\b\|c\]d]] [[note:9223372036854775808|big]] [[note:2|bad\q]]");
        assert!(
            html.contains("data-document-id=\"7\">a\\b|c]d</button>"),
            "{html}"
        );
        assert!(html.contains("[[note:9223372036854775808|big]]"));
        assert!(html.contains(r"[[note:2|bad\q]]"));
    }

    #[test]
    fn renders_multiple_references_with_surrounding_text() {
        let html = render("Start [[note:1|one]] between [[note:2|two]] end");
        assert!(html.contains("Start <button type=\"button\" data-document-id=\"1\">one</button> between <button type=\"button\" data-document-id=\"2\">two</button> end"), "{html}");
    }

    #[test]
    fn leaves_inline_fenced_indented_and_html_references_literal() {
        let html = render(
            "`[[note:1|inline]]`\n\n```text\n[[note:2|fenced]]\n```\n\n    [[note:3|indented]]\n\n<div>[[note:4|html]]</div>",
        );
        assert!(!html.contains("data-document-id"));
        for id in 1..=4 {
            assert!(html.contains(&format!("[[note:{id}|")));
        }
    }

    #[test]
    fn keeps_mermaid_fences() {
        assert!(render("```mermaid\ngraph TD\n```").contains("class=\"language-mermaid\""));
    }

    #[test]
    fn leaves_invalid_references_as_text() {
        assert!(render("[[note:0|bad]]").contains("[[note:0|bad]]"));
    }
}
