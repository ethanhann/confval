//! The diagnostics handler against the fixtures, across formats: pipeline
//! issues mapped to their exact ranges, parse errors for malformed roots,
//! spanless warnings, sibling-element isolation, and position mapping past
//! non-ASCII text.

mod fixture;
mod support;

use std::str::FromStr;

use lsp_types::{DiagnosticSeverity, Position, Uri};

use confval::prelude::{Located, Report, Validate};
use confval::schema::ToSchema;
use confval_lsp::handlers::diagnostics;
use confval_lsp::{Frontend, Hcl, Json, PositionEncoding, Yaml};

use fixture::{GatewaySpec, ServerSpec};
use support::ENCODING;

/// Runs the full parse-then-diagnose path the server runs, for the tests that
/// start from text.
fn full_diagnostics<S, F>(
    frontend: &F,
    schema: &confval::schema::Schema,
    text: &str,
    uri: &lsp_types::Uri,
    encoding: PositionEncoding,
) -> Vec<lsp_types::Diagnostic>
where
    S: confval::format::FromFields
        + confval::pipeline::Validate
        + confval::pipeline::ValidateNested
        + confval::schema::ToSchema,
    F: Frontend,
{
    let (tree, report) = frontend.parse_buffer(text);
    diagnostics(
        confval_lsp::Validator::of::<S>(),
        schema,
        tree.as_ref(),
        &report,
        uri,
        text,
        encoding,
    )
}

#[test]
fn diagnostics_report_the_pipeline_issues_at_their_ranges() {
    // Arrange
    let text = "hostname = \"api\"\nport = 99999\nbogus = 1\nlimits {\n  mode = \"nope\"\n}\n";
    let uri = Uri::from_str("file:///fixture.hcl").unwrap();

    // Act
    let found =
        full_diagnostics::<ServerSpec, _>(&Hcl, &ServerSpec::schema(), text, &uri, ENCODING);

    // Assert
    let messages: Vec<&str> = found.iter().map(|d| d.message.as_str()).collect();
    assert!(
        messages.iter().any(|m| m.contains("port")),
        "expected a port range diagnostic, got: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("unknown field: bogus")),
        "expected an unknown-field diagnostic, got: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("mode")),
        "expected a keyword diagnostic, got: {messages:?}"
    );
    // Each diagnostic has the exact range the pipeline produced, start
    // and end, so a wrong span in any of the three would fail here. `bogus`
    // is the whole name on the third line, `99999` the port value on the
    // second, and `"nope"` the quoted keyword inside the block.
    let bogus = found
        .iter()
        .find(|d| d.message.contains("bogus"))
        .expect("an unknown-field diagnostic");
    assert_eq!(
        bogus.range,
        lsp_types::Range {
            start: Position {
                line: 2,
                character: 0
            },
            end: Position {
                line: 2,
                character: 5
            },
        }
    );
    let port = found
        .iter()
        .find(|d| d.message.contains("port"))
        .expect("a port range diagnostic");
    assert_eq!(
        port.range,
        lsp_types::Range {
            start: Position {
                line: 1,
                character: 7
            },
            end: Position {
                line: 1,
                character: 12
            },
        }
    );
    // The keyword help is held as related information, not appended to the
    // message, so the message stays a single clean line.
    let mode = found
        .iter()
        .find(|d| d.message.contains("mode"))
        .expect("a keyword diagnostic");
    assert_eq!(
        mode.range,
        lsp_types::Range {
            start: Position {
                line: 4,
                character: 9
            },
            end: Position {
                line: 4,
                character: 15
            },
        }
    );
    assert!(
        !mode.message.contains("expected one of"),
        "help is not in the message: {}",
        mode.message
    );
    let related = mode
        .related_information
        .as_ref()
        .expect("the help as related information");
    assert!(
        related
            .iter()
            .any(|note| note.message.contains("expected one of: enforce, log, off")),
        "help appears in related information"
    );
}

#[test]
fn a_spanless_warning_maps_to_the_first_line_with_related_information() {
    // Arrange
    // A handwritten validator emits a warning with no primary span but a related
    // span. The diagnostic points at the first line, has the Warning
    // severity, and keeps the related note.
    #[derive(confval::Spec)]
    struct PlainSpec {
        name: Located<String>,
    }
    impl Validate for PlainSpec {
        fn validate(&self, report: &mut Report) {
            report
                .warning("a general warning")
                .related(self.name.span, "declared here")
                .emit();
        }
    }
    let text = "name = \"api\"\n";
    let uri = Uri::from_str("file:///plain.hcl").unwrap();

    // Act
    let found = full_diagnostics::<PlainSpec, _>(&Hcl, &PlainSpec::schema(), text, &uri, ENCODING);

    // Assert
    let warning = found
        .iter()
        .find(|diagnostic| diagnostic.message.contains("general warning"))
        .expect("a warning");
    assert_eq!(warning.severity, Some(DiagnosticSeverity::WARNING));
    assert_eq!(
        warning.range.start,
        Position {
            line: 0,
            character: 0
        }
    );
    let related = warning
        .related_information
        .as_ref()
        .expect("related information");
    assert!(related.iter().any(|note| note.message == "declared here"));
}

#[test]
fn json_diagnostics_report_the_pipeline_issues() {
    // Arrange
    let text = "{\n  \"hostname\": \"api\",\n  \"port\": 99999,\n  \"bogus\": 1,\n  \"limits\": { \"mode\": \"nope\" }\n}\n";
    let uri = Uri::from_str("file:///fixture.json").unwrap();

    // Act
    let found =
        full_diagnostics::<ServerSpec, _>(&Json, &ServerSpec::schema(), text, &uri, ENCODING);

    // Assert
    let messages: Vec<&str> = found.iter().map(|d| d.message.as_str()).collect();
    assert!(
        messages.iter().any(|m| m.contains("port")),
        "expected a port range diagnostic, got: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("unknown field: bogus")),
        "expected an unknown-field diagnostic, got: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("mode")),
        "expected a keyword diagnostic, got: {messages:?}"
    );
}

#[test]
fn json_root_not_an_object_reports_a_parse_error() {
    // Arrange
    // A JSON array root cannot hold named fields, so the pipeline reports it.
    let text = "[]\n";
    let uri = Uri::from_str("file:///fixture.json").unwrap();

    // Act
    let found =
        full_diagnostics::<ServerSpec, _>(&Json, &ServerSpec::schema(), text, &uri, ENCODING);

    // Assert
    assert!(
        found
            .iter()
            .any(|d| d.message.contains("object at the document root")),
        "expected a root parse error, got: {found:?}"
    );
}

#[test]
fn yaml_diagnostics_report_the_pipeline_issues() {
    // Arrange
    let text = "hostname: api\nport: 99999\nbogus: 1\nlimits:\n  mode: nope\n";
    let uri = Uri::from_str("file:///fixture.yaml").unwrap();

    // Act
    let found =
        full_diagnostics::<ServerSpec, _>(&Yaml, &ServerSpec::schema(), text, &uri, ENCODING);

    // Assert
    let messages: Vec<&str> = found.iter().map(|d| d.message.as_str()).collect();
    assert!(
        messages.iter().any(|m| m.contains("port")),
        "expected a port range diagnostic, got: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("unknown field: bogus")),
        "expected an unknown-field diagnostic, got: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("mode")),
        "expected a keyword diagnostic, got: {messages:?}"
    );
}

#[test]
fn yaml_second_document_reports_a_parse_error() {
    // Arrange
    // A YAML stream with a second document cannot hold one configuration, so the
    // pipeline reports it.
    let text = "hostname: api\n---\nfoo: bar\n";
    let uri = Uri::from_str("file:///fixture.yaml").unwrap();

    // Act
    let found =
        full_diagnostics::<ServerSpec, _>(&Yaml, &ServerSpec::schema(), text, &uri, ENCODING);

    // Assert
    assert!(
        found.iter().any(|d| d.message.contains("single document")),
        "expected a second-document parse error, got: {found:?}"
    );
}

#[test]
fn json_diagnostic_range_survives_a_non_ascii_earlier_value() {
    // Arrange
    // A non-ASCII value on an earlier line adds bytes; the port diagnostic on a
    // later line must still map to the right line and column.
    let text = "{\n  \"hostname\": \"café\",\n  \"port\": 99999\n}\n";
    let uri = Uri::from_str("file:///fixture.json").unwrap();

    // Act
    let found =
        full_diagnostics::<ServerSpec, _>(&Json, &ServerSpec::schema(), text, &uri, ENCODING);

    // Assert
    let port = found
        .iter()
        .find(|d| d.message.contains("port"))
        .expect("a port diagnostic");
    assert_eq!(
        port.range.start,
        Position {
            line: 2,
            character: 10
        }
    );
}

#[test]
fn yaml_diagnostic_range_survives_a_non_ascii_earlier_value() {
    // Arrange
    let text = "hostname: café\nport: 99999\n";
    let uri = Uri::from_str("file:///fixture.yaml").unwrap();

    // Act
    let found =
        full_diagnostics::<ServerSpec, _>(&Yaml, &ServerSpec::schema(), text, &uri, ENCODING);

    // Assert
    let port = found
        .iter()
        .find(|d| d.message.contains("port"))
        .expect("a port diagnostic");
    assert_eq!(
        port.range.start,
        Position {
            line: 1,
            character: 6
        }
    );
}

#[test]
fn a_type_error_in_one_element_does_not_diagnose_a_sibling_element() {
    // Arrange
    // An invalid port in the first upstream element is the only diagnostic. The
    // valid port in the second element is not flagged, so a parse failure in one
    // instance does not contaminate a sibling.
    let text = "upstream:\n  - name: c\n    host: c.internal\n    port: assd\n  - name: b\n    host: b.internal\n    port: 8081\n";
    let uri = Uri::from_str("file:///g.yaml").unwrap();

    // Act
    let found =
        full_diagnostics::<GatewaySpec, _>(&Yaml, &GatewaySpec::schema(), text, &uri, ENCODING);

    // Assert
    assert_eq!(found.len(), 1, "one diagnostic: {found:?}");
    assert_eq!(found[0].message, "expected integer, found string");
    assert_eq!(
        found[0].range.start.line, 3,
        "on the invalid port, not the valid one"
    );
}
