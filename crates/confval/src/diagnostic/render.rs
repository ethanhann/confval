//! Rendering of a [`Report`] into human- and machine-readable output.
//!
//! The report data model lives in [`report`](super::report). This module only
//! reads a finished report and formats it against a [`SourceMap`], resolving
//! each span's byte offset into a line and column at render time. Three formats
//! are offered: a compact one-line-per-issue [`render_plain`](Report::render_plain),
//! a colorized rustc-style [`render_pretty`](Report::render_pretty) (feature
//! `color`), and structured [`render_json`](Report::render_json) (feature
//! `serde`).

use crate::diagnostic::Severity;
use crate::diagnostic::report::Report;
#[cfg(feature = "color")]
use crate::source::SourceId;
use crate::source::{Source, SourceMap, Span};
use std::fmt;

fn severity_label(severity: &Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    }
}

fn resolve(sources: &SourceMap, span: Span) -> Option<(&Source, usize, usize)> {
    let source = sources.get(span.source)?;
    let (line, column) = source.line_column(span.start);
    Some((source, line, column))
}

impl Report {
    /// Compact, one-line-per-issue format for CI/scripts.
    pub fn render_plain(&self, sources: &SourceMap, w: &mut impl fmt::Write) -> fmt::Result {
        for issue in self.issues() {
            let severity = severity_label(&issue.severity);
            match issue.span.and_then(|span| resolve(sources, span)) {
                Some((source, line, column)) => writeln!(
                    w,
                    "{}:{}:{}: {}: {}",
                    source.name, line, column, severity, issue.message
                )?,
                None => writeln!(w, "{}: {}", severity, issue.message)?,
            }
            if let Some(help) = &issue.help {
                writeln!(w, "  help: {}", help)?;
            }
            for (span, label) in &issue.related {
                if let Some((source, line, column)) = resolve(sources, *span) {
                    writeln!(
                        w,
                        "  related: {}:{}:{}: {}",
                        source.name, line, column, label
                    )?;
                }
            }
        }
        Ok(())
    }

    /// Colorized, rustc-style output rendered with `annotate-snippets`: a
    /// severity title, a source excerpt with an underline, help text, and
    /// related locations. Issues group by source in first-appearance order, and
    /// issues without a location come last. Each issue renders on its own and is
    /// separated from the next by one blank line.
    #[cfg(feature = "color")]
    pub fn render_pretty(&self, sources: &SourceMap, w: &mut impl fmt::Write) -> fmt::Result {
        use annotate_snippets::Renderer;
        use annotate_snippets::renderer::DecorStyle;

        let renderer = Renderer::styled().decor_style(DecorStyle::Unicode);
        for (position, index) in self.grouped_order().into_iter().enumerate() {
            if position > 0 {
                writeln!(w)?;
            }
            let block = render_issue(&self.issues()[index], sources, &renderer);
            // A trailing newline ends each block, so a caller that prints after
            // the report starts on a fresh line, and a blank line above each
            // block after the first separates the issues.
            writeln!(w, "{block}")?;
        }
        Ok(())
    }

    /// Issue indices, grouped by primary-span source in order of first
    /// appearance, location-less issues last, insertion order within groups.
    #[cfg(feature = "color")]
    fn grouped_order(&self) -> Vec<usize> {
        let issues = self.issues();
        let mut group_of_source = Vec::new();
        let mut keyed: Vec<(usize, usize)> = Vec::with_capacity(issues.len());
        for (index, issue) in issues.iter().enumerate() {
            let key = match issue.span {
                Some(span) => {
                    let position = group_of_source
                        .iter()
                        .position(|known| *known == span.source);
                    match position {
                        Some(group) => group,
                        None => {
                            group_of_source.push(span.source);
                            group_of_source.len() - 1
                        }
                    }
                }
                None => usize::MAX,
            };
            keyed.push((key, index));
        }
        keyed.sort_by_key(|(key, _)| *key);
        keyed.into_iter().map(|(_, index)| index).collect()
    }
}

/// Renders one issue as an `annotate-snippets` group: a severity title, one
/// snippet per source carrying the primary and the related annotations, and the
/// help text. A related span renders whether or not the issue has a primary
/// span, so a spanless issue still shows its related snippets under its title.
#[cfg(feature = "color")]
fn render_issue(
    issue: &crate::diagnostic::report::Issue,
    sources: &SourceMap,
    renderer: &annotate_snippets::Renderer,
) -> String {
    use annotate_snippets::{Annotation, AnnotationKind, Group, Level, Snippet};

    let level = match issue.severity {
        Severity::Error => Level::ERROR,
        Severity::Warning => Level::WARNING,
    };
    let mut group = Group::with_title(level.primary_title(issue.message.as_str()));

    let mut per_source: Vec<(SourceId, Vec<Annotation>)> = Vec::new();
    if let Some(span) = issue.span
        && let Some(source) = sources.get(span.source)
    {
        add_annotation(
            &mut per_source,
            span.source,
            AnnotationKind::Primary.span(snap_range(source, span)),
        );
    }
    for (span, label) in &issue.related {
        if let Some(source) = sources.get(span.source) {
            add_annotation(
                &mut per_source,
                span.source,
                AnnotationKind::Context
                    .span(snap_range(source, *span))
                    .label(label.as_str()),
            );
        }
    }

    for (id, annotations) in per_source {
        if let Some(source) = sources.get(id) {
            group = group.element(
                Snippet::source(source.text.as_str())
                    .path(source.name.as_str())
                    .line_start(1)
                    .annotations(annotations),
            );
        }
    }

    if let Some(help) = &issue.help {
        group = group.element(Level::HELP.message(help.as_str()));
    }

    renderer.render(&[group])
}

/// Appends an annotation to its source's entry, preserving source order so the
/// primary source's snippet renders first.
#[cfg(feature = "color")]
fn add_annotation<'a>(
    per_source: &mut Vec<(SourceId, Vec<annotate_snippets::Annotation<'a>>)>,
    id: SourceId,
    annotation: annotate_snippets::Annotation<'a>,
) {
    match per_source.iter_mut().find(|(known, _)| *known == id) {
        Some((_, annotations)) => annotations.push(annotation),
        None => per_source.push((id, vec![annotation])),
    }
}

/// A span as a byte range into its source, snapped to character boundaries and
/// clamped to the text, so the range the library slices never splits a
/// character. The range is not clamped to one line, so a multi-line span
/// underlines every line it covers.
#[cfg(feature = "color")]
fn snap_range(source: &Source, span: Span) -> std::ops::Range<usize> {
    let len = source.text.len();
    let start = source.floor_char_boundary((span.start as usize).min(len));
    let end = source
        .ceil_char_boundary((span.end as usize).min(len))
        .max(start);
    start..end
}

#[cfg(feature = "serde")]
impl Report {
    /// Structured JSON for tooling, with resolved line/column alongside raw
    /// byte offsets.
    pub fn render_json(&self, sources: &SourceMap, w: &mut impl fmt::Write) -> fmt::Result {
        #[derive(serde::Serialize)]
        struct LocationJson<'a> {
            source: &'a str,
            line: usize,
            column: usize,
            start: u32,
            end: u32,
        }

        #[derive(serde::Serialize)]
        struct RelatedJson<'a> {
            #[serde(skip_serializing_if = "Option::is_none")]
            location: Option<LocationJson<'a>>,
            label: &'a str,
        }

        #[derive(serde::Serialize)]
        struct IssueJson<'a> {
            severity: &'a str,
            message: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            location: Option<LocationJson<'a>>,
            #[serde(skip_serializing_if = "Option::is_none")]
            help: Option<&'a str>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            related: Vec<RelatedJson<'a>>,
        }

        #[derive(serde::Serialize)]
        struct ReportJson<'a> {
            issues: Vec<IssueJson<'a>>,
        }

        fn location_json<'a>(sources: &'a SourceMap, span: Span) -> Option<LocationJson<'a>> {
            let (source, line, column) = resolve(sources, span)?;
            Some(LocationJson {
                source: &source.name,
                line,
                column,
                start: span.start,
                end: span.end,
            })
        }

        let issues = self
            .issues()
            .iter()
            .map(|issue| IssueJson {
                severity: severity_label(&issue.severity),
                message: &issue.message,
                location: issue.span.and_then(|span| location_json(sources, span)),
                help: issue.help.as_deref(),
                related: issue
                    .related
                    .iter()
                    .map(|(span, label)| RelatedJson {
                        location: location_json(sources, *span),
                        label,
                    })
                    .collect(),
            })
            .collect();

        let rendered =
            serde_json::to_string_pretty(&ReportJson { issues }).map_err(|_| fmt::Error)?;
        w.write_str(&rendered)
    }
}

#[cfg(test)]
mod tests {
    use crate::diagnostic::Report;
    use crate::source::{SourceId, SourceMap, Span};

    fn one_source() -> (SourceMap, SourceId) {
        let mut sources = SourceMap::new();
        let id = sources.add("test.hcl", "port = 99999\nname = \"api\"\n");
        (sources, id)
    }

    #[test]
    fn render_plain_includes_location_and_help() {
        let (sources, id) = one_source();
        let mut report = Report::new();
        report
            .error("port out of range")
            .at(Span::new(id, 7, 12))
            .help("use 1-65535")
            .emit();

        let mut out = String::new();
        report.render_plain(&sources, &mut out).unwrap();
        assert!(
            out.contains("test.hcl:1:8: error: port out of range"),
            "got: {out}"
        );
        assert!(out.contains("  help: use 1-65535"), "got: {out}");
    }

    #[test]
    fn render_plain_without_location() {
        let sources = SourceMap::new();
        let mut report = Report::new();
        report.error("no ingress files found").emit();

        let mut out = String::new();
        report.render_plain(&sources, &mut out).unwrap();
        assert_eq!(out, "error: no ingress files found\n");
    }

    #[test]
    fn render_plain_includes_related_locations() {
        let mut sources = SourceMap::new();
        let a = sources.add("a.hcl", "bind = \"127.0.0.1:80\"\n");
        let b = sources.add("b.hcl", "bind = \"127.0.0.1:80\"\n");
        let mut report = Report::new();
        report
            .error("duplicate bind address")
            .at(Span::new(b, 0, 4))
            .related(Span::new(a, 0, 4), "first declared here")
            .emit();

        let mut out = String::new();
        report.render_plain(&sources, &mut out).unwrap();
        assert!(
            out.contains("b.hcl:1:1: error: duplicate bind address"),
            "got: {out}"
        );
        assert!(
            out.contains("  related: a.hcl:1:1: first declared here"),
            "got: {out}"
        );
    }

    #[cfg(feature = "color")]
    #[test]
    fn render_pretty_underlines_the_span() {
        // Arrange
        // The span covers "99999" on line 1, at columns 8 to 12.
        let (sources, id) = one_source();
        let mut report = Report::new();
        report
            .error("port out of range")
            .at(Span::new(id, 7, 12))
            .emit();

        // Act
        let rendered = pretty(&sources, &report);

        // Assert
        assert_eq!(
            rendered,
            "error: port out of range\n  ╭▸ test.hcl:1:8\n  │\n1 │ port = 99999\n  ╰╴       ━━━━━\n"
        );
    }

    /// Strips ANSI escapes so an assertion can read the rendered text itself.
    /// The `color` feature wraps the gutter and the underline in style codes.
    #[cfg(feature = "color")]
    fn without_ansi(text: &str) -> String {
        let mut out = String::new();
        let mut chars = text.chars();
        while let Some(character) = chars.next() {
            if character != '\u{1b}' {
                out.push(character);
                continue;
            }
            for escape in chars.by_ref() {
                if escape == 'm' {
                    break;
                }
            }
        }
        out
    }

    /// Renders a report to its ANSI-stripped pretty block, the golden the pretty
    /// tests assert against.
    #[cfg(feature = "color")]
    fn pretty(sources: &SourceMap, report: &Report) -> String {
        let mut out = String::new();
        report.render_pretty(sources, &mut out).unwrap();
        without_ansi(&out)
    }

    #[cfg(feature = "color")]
    #[test]
    fn render_pretty_aligns_the_underline_under_its_span() {
        // Arrange
        // The golden block guards alignment: the underline sits under "99999",
        // seven columns past the gutter, which a caret-run check alone would not
        // catch.
        let (sources, id) = one_source();
        let mut report = Report::new();
        report
            .error("port out of range")
            .at(Span::new(id, 7, 12))
            .emit();

        // Act
        let rendered = pretty(&sources, &report);

        // Assert
        assert_eq!(
            rendered,
            "error: port out of range\n  ╭▸ test.hcl:1:8\n  │\n1 │ port = 99999\n  ╰╴       ━━━━━\n"
        );
    }

    #[cfg(feature = "color")]
    #[test]
    fn render_pretty_span_inside_multibyte_char_does_not_panic() {
        // "é" is two bytes. An offset..offset+1 span ending mid-character must
        // not panic the underline slice.
        let mut sources = SourceMap::new();
        let id = sources.add("test.hcl", "x = é\n");
        let bad_end = "x = é".find('é').unwrap() as u32 + 1;
        assert!(
            !sources
                .get(id)
                .unwrap()
                .text
                .is_char_boundary(bad_end as usize)
        );
        let mut report = Report::new();
        report
            .error("syntax error")
            .at(Span::new(id, bad_end - 1, bad_end))
            .emit();

        let mut out = String::new();
        report.render_pretty(&sources, &mut out).unwrap();
        assert!(out.contains("syntax error"), "got: {out}");
    }

    #[cfg(feature = "color")]
    #[test]
    fn render_pretty_cross_file_related_span() {
        // Arrange
        let mut sources = SourceMap::new();
        let a = sources.add("a.hcl", "bind = \"127.0.0.1:80\"\n");
        let b = sources.add("b.hcl", "bind = \"127.0.0.1:80\"\n");
        let mut report = Report::new();
        report
            .error("duplicate bind address")
            .at(Span::new(b, 7, 21))
            .related(Span::new(a, 7, 21), "first declared here")
            .emit();

        // Act
        let rendered = pretty(&sources, &report);

        // Assert
        // The primary snippet in b.hcl carries the heavy underline, and the
        // related snippet in a.hcl carries the light underline with its label.
        assert_eq!(
            rendered,
            "error: duplicate bind address\n  ╭▸ b.hcl:1:8\n  │\n1 │ bind = \"127.0.0.1:80\"\n  │        ━━━━━━━━━━━━━━\n  │\n  ⸬  a.hcl:1:8\n  │\n1 │ bind = \"127.0.0.1:80\"\n  ╰╴       ────────────── first declared here\n"
        );
    }

    #[cfg(feature = "color")]
    #[test]
    fn render_pretty_renders_a_warning() {
        // Arrange
        let mut sources = SourceMap::new();
        let id = sources.add("test.hcl", "host = \"\"\n");
        let mut report = Report::new();
        report
            .warning("host is empty")
            .at(Span::new(id, 7, 9))
            .emit();

        // Act
        let rendered = pretty(&sources, &report);

        // Assert
        assert_eq!(
            rendered,
            "warning: host is empty\n  ╭▸ test.hcl:1:8\n  │\n1 │ host = \"\"\n  ╰╴       ━━\n"
        );
    }

    #[cfg(feature = "color")]
    #[test]
    fn render_pretty_renders_help_after_the_related_annotation() {
        // Arrange
        let (sources, id) = one_source();
        let mut report = Report::new();
        report
            .error("port out of range")
            .at(Span::new(id, 7, 12))
            .help("use 1 to 65535")
            .related(Span::new(id, 13, 17), "the name field")
            .emit();

        // Act
        let rendered = pretty(&sources, &report);

        // Assert
        // The help line renders after the snippet and its annotations.
        assert_eq!(
            rendered,
            "error: port out of range\n  ╭▸ test.hcl:1:8\n  │\n1 │ port = 99999\n  │        ━━━━━\n2 │ name = \"api\"\n  │ ──── the name field\n  │\n  ╰ help: use 1 to 65535\n"
        );
    }

    #[cfg(feature = "color")]
    #[test]
    fn render_pretty_underlines_a_multiline_span() {
        // Arrange
        // The span runs from the start of line 1 into line 2, so the underline
        // covers both lines rather than the first alone.
        let mut sources = SourceMap::new();
        let id = sources.add("test.hcl", "a = 1\nb = 2\n");
        let mut report = Report::new();
        report
            .error("spans two lines")
            .at(Span::new(id, 0, 11))
            .emit();

        // Act
        let rendered = pretty(&sources, &report);

        // Assert
        assert_eq!(
            rendered,
            "error: spans two lines\n  ╭▸ test.hcl:1:1\n  │\n1 │ ┏ a = 1\n2 │ ┃ b = 2\n  ╰╴┗━━━━━┛\n"
        );
    }

    #[cfg(feature = "color")]
    #[test]
    fn render_pretty_groups_by_source_first_appearance() {
        let mut sources = SourceMap::new();
        let a = sources.add("a.hcl", "x = 1\n");
        let b = sources.add("b.hcl", "y = 2\n");
        let mut report = Report::new();
        report.error("first in a").at(Span::new(a, 0, 1)).emit();
        report.error("only in b").at(Span::new(b, 0, 1)).emit();
        report.error("second in a").at(Span::new(a, 4, 5)).emit();
        report.error("no location").emit();

        let mut out = String::new();
        report.render_pretty(&sources, &mut out).unwrap();
        let first_a = out.find("first in a").unwrap();
        let second_a = out.find("second in a").unwrap();
        let only_b = out.find("only in b").unwrap();
        let unlocated = out.find("no location").unwrap();
        assert!(first_a < second_a, "a-issues stay adjacent: {out}");
        assert!(second_a < only_b, "a-group precedes b-group: {out}");
        assert!(only_b < unlocated, "location-less issues come last: {out}");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn render_json_resolves_locations() {
        let (sources, id) = one_source();
        let mut report = Report::new();
        report
            .error("port out of range")
            .at(Span::new(id, 7, 12))
            .help("use 1-65535")
            .emit();

        let mut out = String::new();
        report.render_json(&sources, &mut out).unwrap();
        let value: serde_json::Value = serde_json::from_str(&out).unwrap();
        let issue = &value["issues"][0];
        assert_eq!(issue["severity"], "error");
        assert_eq!(issue["message"], "port out of range");
        assert_eq!(issue["location"]["source"], "test.hcl");
        assert_eq!(issue["location"]["line"], 1);
        assert_eq!(issue["location"]["column"], 8);
        assert_eq!(issue["location"]["start"], 7);
        assert_eq!(issue["help"], "use 1-65535");
    }
}
