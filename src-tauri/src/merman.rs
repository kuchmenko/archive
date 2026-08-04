use merman::render::{
    HeadlessRenderer, HostThemeOutput, HostThemeProfile, HostThemeRoles, HostThemeRootBackground,
};
use merman_analysis::{AnalysisOptions, Analyzer, DiagnosticSeverity};
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use rmcp::schemars;
use serde::Serialize;

pub const MAX_MERMAID_SOURCE_BYTES: usize = 100 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct MermaidDiagnostic {
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct MermaidResult {
    pub valid: bool,
    pub diagram_type: Option<String>,
    pub diagnostics: Vec<MermaidDiagnostic>,
    pub svg: Option<String>,
}

fn engine(diagram_id: Option<&str>) -> (Analyzer, HeadlessRenderer) {
    let mut output = HostThemeOutput::resvg_safe_editor();
    output.root_background = HostThemeRootBackground::Color("transparent".to_owned());
    let profile = HostThemeProfile::builder()
        .roles(HostThemeRoles {
            canvas: Some("#1b1e24".into()),
            surface: Some("#1b1e24".into()),
            surface_alt: Some("#232831".into()),
            surface_muted: Some("#232831".into()),
            text: Some("#ece8df".into()),
            subtle_text: Some("#b8b3aa".into()),
            border: Some("#4a5260".into()),
            line: Some("#8f98a7".into()),
            edge_label_background: Some("#1b1e24".into()),
            cluster_background: Some("#232831".into()),
            cluster_border: Some("#4a5260".into()),
            note_background: Some("#302a20".into()),
            note_border: Some("#9d8555".into()),
            note_text: Some("#ece8df".into()),
            actor_background: Some("#232831".into()),
            actor_border: Some("#4a5260".into()),
            actor_text: Some("#ece8df".into()),
            activation_background: Some("#232831".into()),
            activation_border: Some("#8f98a7".into()),
            ..HostThemeRoles::default()
        })
        .theme_variable("background", "transparent")
        .output(output)
        .build();
    let mut renderer = HeadlessRenderer::new()
        .with_strict_parsing()
        .with_host_theme(&profile);
    if let Some(diagram_id) = diagram_id {
        renderer = renderer.with_diagram_id(diagram_id);
    }
    let options = AnalysisOptions::default().with_max_source_bytes(Some(MAX_MERMAID_SOURCE_BYTES));
    let analyzer = Analyzer::with_engine_and_options(renderer.engine.clone(), options);
    (analyzer, renderer)
}

pub fn validate(source: &str) -> MermaidResult {
    validate_with_svg(source, None)
}

pub fn render(source: &str, diagram_id: &str) -> MermaidResult {
    validate_with_svg(source, Some(diagram_id))
}

fn validate_with_svg(source: &str, diagram_id: Option<&str>) -> MermaidResult {
    let (analyzer, renderer) = engine(diagram_id);
    let analysis = analyzer.analyze_result(source);
    let diagram_type = analysis
        .diagrams()
        .first()
        .and_then(|diagram| diagram.syntax.diagram_type.clone())
        .or_else(|| {
            analysis
                .diagnostics()
                .iter()
                .find_map(|diagnostic| diagnostic.diagram_type.clone())
        });
    let mut diagnostics = analysis
        .diagnostics()
        .iter()
        .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
        .map(|diagnostic| MermaidDiagnostic {
            line: diagnostic.span.as_ref().map(|span| span.line),
            column: diagnostic.span.as_ref().map(|span| span.column),
            message: diagnostic.message.clone(),
        })
        .collect::<Vec<_>>();
    let mut valid = analysis.payload().valid;
    let svg = if valid && diagram_id.is_some() {
        match renderer.render_svg_sync(source) {
            Ok(Some(svg)) => Some(svg),
            Ok(None) => {
                valid = false;
                diagnostics.push(MermaidDiagnostic {
                    line: None,
                    column: None,
                    message: "source does not contain a Mermaid diagram".to_owned(),
                });
                None
            }
            Err(error) => {
                valid = false;
                diagnostics.push(MermaidDiagnostic {
                    line: None,
                    column: None,
                    message: error.to_string(),
                });
                None
            }
        }
    } else {
        None
    };
    MermaidResult {
        valid,
        diagram_type,
        diagnostics,
        svg,
    }
}

pub fn validate_markdown_fences(markdown: &str) -> Result<(), String> {
    let mut block = None;
    let mut block_index = 0;
    for (event, range) in Parser::new_ext(markdown, Options::empty()).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info)))
                if info
                    .split_whitespace()
                    .next()
                    .is_some_and(|language| language.eq_ignore_ascii_case("mermaid")) =>
            {
                block_index += 1;
                let opening = markdown[range.clone()].lines().next().unwrap_or("");
                let delimiter = opening.trim_start().as_bytes()[0];
                let delimiter_len = opening
                    .trim_start()
                    .bytes()
                    .take_while(|byte| *byte == delimiter)
                    .count();
                block = Some((block_index, range, String::new(), delimiter, delimiter_len));
            }
            Event::Text(text) => {
                if let Some((_, _, source, _, _)) = &mut block {
                    source.push_str(&text);
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                let Some((index, range, source, delimiter, delimiter_len)) = block.take() else {
                    continue;
                };
                let last_line = markdown[range].lines().next_back().unwrap_or("");
                let indent = last_line.bytes().take_while(|byte| *byte == b' ').count();
                let closing = last_line
                    .get(indent..)
                    .filter(|_| indent <= 3)
                    .unwrap_or("")
                    .trim_end();
                let closed =
                    closing.len() >= delimiter_len && closing.bytes().all(|byte| byte == delimiter);
                if !closed {
                    return Err(format!("Mermaid block {index} is unclosed"));
                }
                validate_markdown_block(index, &source)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_markdown_block(index: usize, source: &str) -> Result<(), String> {
    let result = validate(source);
    if !result.valid {
        let details = result
            .diagnostics
            .iter()
            .map(|diagnostic| match (diagnostic.line, diagnostic.column) {
                (Some(line), Some(column)) => {
                    format!("line {line}, column {column}: {}", diagnostic.message)
                }
                _ => diagnostic.message.clone(),
            })
            .collect::<Vec<_>>()
            .join("; ");
        return Err(format!("Mermaid block {index} is invalid: {details}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_and_renders_flowchart_and_sequence() {
        for (source, kind) in [
            ("flowchart TD\nA-->B", "flowchart-v2"),
            ("sequenceDiagram\nAlice->>Bob: Hi", "sequence"),
        ] {
            let result = render(source, "archive-test");
            assert!(result.valid, "{:?}", result.diagnostics);
            assert_eq!(result.diagram_type.as_deref(), Some(kind));
            assert!(
                result
                    .svg
                    .as_deref()
                    .is_some_and(|svg| svg.contains("<svg"))
            );
            let svg = result.svg.unwrap().to_lowercase();
            assert!(svg.contains("#1b1e24") || svg.contains("#232831"));
            assert!(svg.contains("#ece8df"));
            for opaque in [
                "background-color:white",
                "background-color:#fff",
                "background-color:#ffffff",
                "rgb(255",
            ] {
                assert!(!svg.contains(opaque), "{svg}");
            }
        }
        let first = render("flowchart TD\nA-->B", "archive-first").svg.unwrap();
        let second = render("flowchart TD\nA-->B", "archive-second").svg.unwrap();
        assert!(first.contains("archive-first"));
        assert!(second.contains("archive-second"));
        assert_ne!(first, second);
    }

    #[test]
    fn invalid_sources_have_structured_diagnostics_and_limit() {
        for source in ["flowchart TD\nA-->", "sequenceDiagram\nAlice->>"] {
            let result = validate(source);
            assert!(!result.valid);
            assert!(!result.diagnostics.is_empty());
            assert!(result.diagnostics.iter().any(|item| item.line.is_some()));
        }
        let result = validate(&"x".repeat(MAX_MERMAID_SOURCE_BYTES + 1));
        assert!(!result.valid);
        assert!(result.diagnostics[0].message.contains("102400"));
    }

    #[test]
    fn scans_commonmark_mermaid_fences_and_fails_closed() {
        assert!(validate_markdown_fences("```js\nbad\n```\n:::mermaid\nbad\n:::").is_ok());
        assert!(validate_markdown_fences("```mermaid `example`\nflowchart TD\nA-->\n```").is_ok());
        assert!(validate_markdown_fences("~~~mermaid\nflowchart TD\nA-->B\n~~~~\n").is_ok());
        assert!(validate_markdown_fences("````mermaid\nflowchart TD\nA-->B\n````\n").is_ok());
        let multiple = "```mermaid\nflowchart TD\nA-->B\n```\n~~~mermaid\nflowchart TD\nA-->\n~~~";
        assert!(
            validate_markdown_fences(multiple)
                .unwrap_err()
                .contains("block 2")
        );
        assert!(
            validate_markdown_fences("```mermaid\nflowchart TD\nA-->B")
                .unwrap_err()
                .contains("unclosed")
        );
        assert!(
            validate_markdown_fences("```mermaid\nflowchart TD\nA-->B\n    ```")
                .unwrap_err()
                .contains("unclosed")
        );
        assert!(
            validate_markdown_fences("````mermaid\nflowchart TD\nA-->B\n```")
                .unwrap_err()
                .contains("unclosed")
        );
    }
}
