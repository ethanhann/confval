//! The rendered-default consumers: the hover value, the pre-filled insert, the
//! preselected value item, and the reset-to-default quick fix.

mod fixture;

use std::str::FromStr;

use lsp_types::{
    CodeActionKind, CodeActionOrCommand, CompletionItem, CompletionTextEdit, HoverContents,
    InsertTextFormat, Uri,
};

use confval::format::{FieldKind, Scalar, ValueKind};
use confval::prelude::{Located, Report, Validate};
use confval::schema::ToSchema;
use confval::{length_constraint, range_constraint};
use confval_lsp::handlers::{ClientSupport, Cx, code_action, completion, diagnostics, hover};
use confval_lsp::{Frontend, Hcl, Json, Kdl, LineIndex, PositionEncoding, Toml, Yaml};
use std::path::PathBuf;

use fixture::{LimitsSpec, ServerSpec};

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

const ENCODING: PositionEncoding = PositionEncoding::Utf8;

/// The test document URI.
fn doc_uri() -> Uri {
    match Uri::from_str("file:///doc") {
        Ok(uri) => uri,
        Err(_) => panic!("a valid uri"),
    }
}

/// A spec whose string default contains snippet metacharacters.
#[derive(confval::Spec)]
struct BadgeSpec {
    /// A default with `$`, `{`, and `}` in it.
    #[confval(default = "${HOME}/x".to_string())]
    home: Located<String>,
}

impl Validate for BadgeSpec {
    fn validate(&self, _report: &mut Report) {}
}

/// The text a completion item inserts, read from its replace edit.
fn inserted(item: &CompletionItem) -> String {
    match &item.text_edit {
        Some(CompletionTextEdit::Edit(edit)) => edit.new_text.clone(),
        _ => item.insert_text.clone().unwrap_or_default(),
    }
}

/// Runs completion at an offset with the given client flags.
fn complete<F: Frontend>(
    frontend: &F,
    schema: &confval::schema::Schema,
    text: &str,
    offset: usize,
    snippets: bool,
    preselect: bool,
) -> Vec<CompletionItem> {
    let tree = frontend.parse_tree(text);
    let context = frontend.resolve(tree.as_ref(), text, offset);
    let index = LineIndex::new(text);
    completion(
        frontend,
        &Cx {
            schema,
            fields: tree.as_ref(),
            ctx: &context,
            text,
        },
        &index,
        ENCODING,
        ClientSupport::new(snippets, preselect),
    )
}

/// The hover markdown at an offset against a schema.
fn hover_markdown<F: Frontend>(
    frontend: &F,
    schema: &confval::schema::Schema,
    text: &str,
    offset: usize,
) -> String {
    let tree = frontend.parse_tree(text);
    let context = frontend.resolve(tree.as_ref(), text, offset);
    let index = LineIndex::new(text);
    let Some(found) = hover(
        &Cx {
            schema,
            fields: tree.as_ref(),
            ctx: &context,
            text,
        },
        &index,
        ENCODING,
    ) else {
        panic!("a hover is produced");
    };
    match found.contents {
        HoverContents::Markup(markup) => markup.value,
        _ => panic!("expected a markdown hover"),
    }
}

/// Runs the code-action handler at an offset with the given diagnostics.
fn actions_at<F: Frontend>(
    frontend: &F,
    schema: &confval::schema::Schema,
    text: &str,
    offset: usize,
    diagnostics: &[lsp_types::Diagnostic],
    only: Option<&[CodeActionKind]>,
) -> Vec<CodeActionOrCommand> {
    let tree = frontend.parse_tree(text);
    let context = frontend.resolve(tree.as_ref(), text, offset);
    let uri = doc_uri();
    let index = LineIndex::new(text);
    code_action(
        frontend,
        &Cx {
            schema,
            fields: tree.as_ref(),
            ctx: &context,
            text,
        },
        diagnostics,
        only,
        &uri,
        &index,
        ENCODING,
    )
}

/// The real pipeline diagnostics for a YAML ServerSpec document.
fn server_diagnostics(text: &str) -> Vec<lsp_types::Diagnostic> {
    full_diagnostics::<ServerSpec, _>(&Yaml, &ServerSpec::schema(), text, &doc_uri(), ENCODING)
}

#[test]
fn hover_prints_the_rendered_default_for_the_three_states() {
    // Arrange
    let schema = ServerSpec::schema();
    let set = "hostname: h\nport: 1\nworkers: 2\n";
    let unset = "hostname: h\nport: 1\nlimits:\n  mode\n";
    // The half-typed `allow` does not parse, so the field is not set and the
    // uncarried default line renders.
    let uncarried = "hostname: h\nport: 1\nallow";

    // Act
    let set_markdown = hover_markdown(&Yaml, &schema, set, set.find("workers").unwrap() + 1);
    let unset_markdown = hover_markdown(&Yaml, &schema, unset, unset.find("mode").unwrap() + 1);
    let uncarried_markdown = hover_markdown(
        &Yaml,
        &schema,
        uncarried,
        uncarried.find("allow").unwrap() + 1,
    );

    // Assert
    assert!(
        !set_markdown.contains("Defaults to"),
        "a set field states its state alone: {set_markdown}"
    );
    assert!(
        set_markdown.contains("Set by the configuration."),
        "{set_markdown}"
    );
    assert!(
        unset_markdown.contains("Defaults to \"enforce\"."),
        "{unset_markdown}"
    );
    assert!(
        unset_markdown.contains("Not set. Uses its default."),
        "{unset_markdown}"
    );
    assert!(
        uncarried_markdown.contains("Has a default."),
        "a defaulted list keeps the current line: {uncarried_markdown}"
    );
    assert!(
        !uncarried_markdown.contains("Defaults to"),
        "{uncarried_markdown}"
    );
}

#[test]
fn the_insert_pre_fills_the_default_as_a_selected_placeholder() {
    // Arrange
    let schema = ServerSpec::schema();
    let text = "";

    // Act
    let with = complete(&Yaml, &schema, text, 0, true, false);
    let without = complete(&Yaml, &schema, text, 0, false, false);

    // Assert
    let snippet = with.iter().find(|i| i.label == "workers").expect("workers");
    assert_eq!(inserted(snippet), "workers: ${1:4}");
    assert_eq!(snippet.insert_text_format, Some(InsertTextFormat::SNIPPET));
    let plain = without
        .iter()
        .find(|i| i.label == "workers")
        .expect("workers");
    assert_eq!(inserted(plain), "workers: 4", "the placeholder unwraps");
    assert_eq!(plain.insert_text_format, None);
}

#[test]
fn an_adversarial_default_is_snippet_escaped_and_unwraps_clean() {
    // Arrange
    let schema = BadgeSpec::schema();
    let text = "";

    // Act
    let with = complete(&Hcl, &schema, text, 0, true, false);
    let without = complete(&Hcl, &schema, text, 0, false, false);

    // Assert
    let snippet = inserted(with.iter().find(|i| i.label == "home").expect("home"));
    assert!(
        snippet.contains("\\$") && snippet.contains("\\}"),
        "the metacharacters are escaped: {snippet}"
    );
    let plain = inserted(without.iter().find(|i| i.label == "home").expect("home"));
    assert_eq!(plain, "home = \"${HOME}/x\"", "the escaping unwinds");
}

#[test]
fn a_value_position_offers_the_preselected_default_per_constraint_shape() {
    // Arrange
    // `mode` has keywords, so the default's item is preselected with no
    // duplicate. `workers` has a range, so the rendered default is the one
    // offered item.
    let schema = ServerSpec::schema();
    let keyword_text = "limits:\n  mode: \n";
    let keyword_offset = keyword_text.find("mode: ").unwrap() + "mode: ".len();
    let range_text = "workers: \n";
    let range_offset = "workers: ".len();

    // Act
    let keywords = complete(&Yaml, &schema, keyword_text, keyword_offset, false, true);
    let ungated = complete(&Yaml, &schema, keyword_text, keyword_offset, false, false);
    let range = complete(&Yaml, &schema, range_text, range_offset, false, true);

    // Assert
    let enforce = keywords
        .iter()
        .find(|i| i.label == "enforce")
        .expect("enforce");
    assert_eq!(
        enforce.preselect,
        Some(true),
        "the default keyword preselects"
    );
    assert!(
        keywords
            .iter()
            .filter(|i| i.label != "enforce")
            .all(|i| i.preselect.is_none()),
        "the other keywords stay unselected"
    );
    assert!(
        ungated.iter().all(|i| i.preselect.is_none()),
        "no preselect without the client capability"
    );
    assert_eq!(range.len(), 1, "the rendered default is the one item");
    assert_eq!(range[0].label, "4");
    assert_eq!(inserted(&range[0]), "4");
    assert_eq!(range[0].preselect, Some(true));
}

/// A spec whose defaults cover every scalar leaf, for the round trips.
#[derive(confval::Spec)]
struct RoundTripSpec {
    /// An integer default.
    #[confval(default = 4)]
    workers: Located<i64>,
    /// A whole-number float default.
    #[confval(default = 4.0)]
    scale: Located<f64>,
    /// A boolean default.
    #[confval(default = true)]
    secure: Located<bool>,
    /// A string default.
    #[confval(default = "enforce".to_string())]
    mode: Located<String>,
    /// A path default.
    #[confval(default = PathBuf::from("/etc/app.conf"))]
    config: Located<PathBuf>,
    /// A default containing control characters, which must render escaped.
    #[confval(default = "a\nb\tc".to_string())]
    banner: Located<String>,
}

impl Validate for RoundTripSpec {
    fn validate(&self, _report: &mut Report) {}
}

/// The parsed scalar of a field, for comparing a round trip against the
/// default it inserted.
fn parsed_scalar(tree: &confval::format::Fields, name: &str) -> Scalar {
    let field = tree.get(name).unwrap_or_else(|| panic!("{name} parses"));
    match &field.kind {
        FieldKind::Value(value) => match &value.kind {
            ValueKind::Scalar(scalar) => scalar.clone(),
            other => panic!("{name} holds a scalar, got {other:?}"),
        },
        other => panic!("{name} holds a value, got {other:?}"),
    }
}

#[test]
fn the_rendered_default_round_trips_through_every_frontend() {
    // Arrange
    // Completing every defaulted leaf on an empty document and parsing the
    // result back yields the default values, so each frontend's literal form
    // is one its parser reads.
    let schema = RoundTripSpec::schema();

    fn round_trip<F: Frontend>(
        frontend: &F,
        schema: &confval::schema::Schema,
        wrap: impl Fn(&str) -> String,
    ) {
        // Act
        let items = complete(frontend, schema, "", 0, false, false);
        let field = |name: &str| {
            inserted(
                items
                    .iter()
                    .find(|item| item.label == name)
                    .unwrap_or_else(|| panic!("{name} offered")),
            )
        };
        let body = [
            field("workers"),
            field("scale"),
            field("secure"),
            field("mode"),
            field("config"),
            field("banner"),
        ]
        .join("\n");
        let document = wrap(&body);
        let tree = frontend
            .parse_tree(&document)
            .unwrap_or_else(|| panic!("the completed document parses: {document:?}"));

        // Assert
        assert_eq!(parsed_scalar(&tree, "workers"), Scalar::Int(4));
        assert_eq!(parsed_scalar(&tree, "scale"), Scalar::Float(4.0));
        assert_eq!(parsed_scalar(&tree, "secure"), Scalar::Bool(true));
        assert_eq!(
            parsed_scalar(&tree, "mode"),
            Scalar::String("enforce".to_string())
        );
        assert_eq!(
            parsed_scalar(&tree, "config"),
            Scalar::String("/etc/app.conf".to_string())
        );
        assert_eq!(
            parsed_scalar(&tree, "banner"),
            Scalar::String("a\nb\tc".to_string())
        );
    }
    round_trip(&Hcl, &schema, |body| format!("{body}\n"));
    round_trip(&Toml, &schema, |body| format!("{body}\n"));
    round_trip(&Kdl, &schema, |body| format!("{body}\n"));
    round_trip(&Yaml, &schema, |body| format!("{body}\n"));
    round_trip(&Json, &schema, |body| {
        format!("{{ {} }}", body.replace('\n', ", "))
    });
}

#[test]
fn the_kdl_boolean_default_renders_its_hash_form() {
    // Arrange
    let schema = ServerSpec::schema();

    // Act
    let items = complete(&Kdl, &schema, "", 0, false, false);

    // Assert
    let tls = inserted(items.iter().find(|i| i.label == "tls").expect("tls"));
    assert_eq!(tls, "tls #false");
}

#[test]
fn the_string_default_round_trips_inside_the_block() {
    // Arrange
    let schema = LimitsSpec::schema();

    // Act
    let items = complete(&Yaml, &schema, "", 0, false, false);

    // Assert
    let mode = inserted(items.iter().find(|i| i.label == "mode").expect("mode"));
    assert_eq!(mode, "mode: \"enforce\"");
    let tree = Yaml.parse_tree(&format!("{mode}\n")).expect("parses");
    assert!(tree.has("mode"));
}

#[test]
fn a_range_violation_gets_the_reset_quick_fix() {
    // Arrange
    let text = "hostname: h\nport: 1\nworkers: 9999\n";
    let found = server_diagnostics(text);
    let offset = text.find("9999").unwrap() + 1;
    let schema = ServerSpec::schema();

    // Act
    let actions = actions_at(&Yaml, &schema, text, offset, &found, None);

    // Assert
    let CodeActionOrCommand::CodeAction(action) = actions.first().expect("one quick fix") else {
        panic!("a code action");
    };
    assert_eq!(action.title, "Set workers to the default 4");
    assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
    assert_eq!(action.is_preferred, Some(true));
    assert!(
        action.diagnostics.as_ref().is_some_and(|d| !d.is_empty()),
        "the action carries the diagnostic it fixes"
    );
    let edits: Vec<lsp_types::TextEdit> = action
        .edit
        .as_ref()
        .and_then(|edit| edit.changes.as_ref())
        .map(|changes| changes.values().flatten().cloned().collect())
        .unwrap_or_default();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, "4");
}

#[test]
fn a_keyword_violation_and_a_type_mismatch_get_the_fix() {
    // Arrange
    let keyword = "hostname: h\nport: 1\nlimits:\n  mode: \"loud\"\n";
    let mismatch = "hostname: h\nport: 1\nworkers: \"abc\"\n";
    let schema = ServerSpec::schema();

    // Act
    let keyword_actions = actions_at(
        &Yaml,
        &schema,
        keyword,
        keyword.find("loud").unwrap(),
        &server_diagnostics(keyword),
        None,
    );
    let mismatch_actions = actions_at(
        &Yaml,
        &schema,
        mismatch,
        mismatch.find("abc").unwrap(),
        &server_diagnostics(mismatch),
        None,
    );

    // Assert
    assert_eq!(keyword_actions.len(), 1, "the keyword violation fixes");
    let CodeActionOrCommand::CodeAction(action) = &keyword_actions[0] else {
        panic!("a code action");
    };
    assert_eq!(action.title, "Set mode to the default \"enforce\"");
    assert_eq!(mismatch_actions.len(), 1, "the type mismatch fixes");
}

#[test]
fn a_field_without_a_default_offers_no_fix() {
    // Arrange
    // `port` has no default, so its range violation offers nothing.
    let schema = ServerSpec::schema();
    let text = "hostname: h\nport: 99999\n";

    // Act
    let actions = actions_at(
        &Yaml,
        &schema,
        text,
        text.find("99999").unwrap(),
        &server_diagnostics(text),
        None,
    );

    // Assert
    assert!(actions.is_empty(), "no default, no fix");
}

#[test]
fn an_enclosing_span_diagnostic_offers_no_fix() {
    // Arrange
    // The missing-required diagnostic anchors at the enclosing span, so it
    // does not qualify a defaulted value elsewhere.
    let schema = ServerSpec::schema();
    let text = "port: 1\nworkers: 2\n";

    // Act
    let actions = actions_at(
        &Yaml,
        &schema,
        text,
        text.find('2').unwrap(),
        &server_diagnostics(text),
        None,
    );

    // Assert
    assert!(
        actions.is_empty(),
        "an enclosing-span diagnostic does not qualify"
    );
}

#[test]
fn a_body_position_offers_no_fix() {
    // Arrange
    let schema = ServerSpec::schema();
    let text = "hostname: h\nport: 1\nworkers: 9999\n";

    // Act
    let actions = actions_at(&Yaml, &schema, text, 1, &server_diagnostics(text), None);

    // Assert
    assert!(actions.is_empty(), "a body position does not qualify");
}

#[test]
fn an_empty_diagnostic_context_offers_no_fix() {
    // Arrange
    let schema = ServerSpec::schema();
    let text = "hostname: h\nport: 1\nworkers: 9999\n";
    let offset = "hostname: h\nport: 1\nworkers: 99".len();

    // Act
    let actions = actions_at(&Yaml, &schema, text, offset, &[], None);

    // Assert
    assert!(actions.is_empty(), "an empty context yields nothing");
}

#[test]
fn an_unparsed_buffer_offers_no_fix() {
    // Arrange
    // The edit needs a parsed span to replace, so a buffer with no parse
    // offers nothing.
    let schema = ServerSpec::schema();
    let text = "workers: 9999\nbad: [\n";

    // Act
    let actions = actions_at(
        &Yaml,
        &schema,
        text,
        text.find("9999").unwrap(),
        &server_diagnostics(text),
        None,
    );

    // Assert
    assert!(actions.is_empty(), "the edit needs a parsed span");
}

#[test]
fn a_non_quickfix_kind_filter_suppresses_the_fix() {
    // Arrange
    let schema = ServerSpec::schema();
    let text = "hostname: h\nport: 1\nworkers: 9999\n";
    let offset = "hostname: h\nport: 1\nworkers: 99".len();

    // Act
    let actions = actions_at(
        &Yaml,
        &schema,
        text,
        offset,
        &server_diagnostics(text),
        Some(&[CodeActionKind::REFACTOR]),
    );

    // Assert
    assert!(actions.is_empty(), "the kind filter is honored");
}

#[test]
fn a_quickfix_kind_filter_passes_the_fix() {
    // Arrange
    let schema = ServerSpec::schema();
    let text = "hostname: h\nport: 1\nworkers: 9999\n";
    let offset = "hostname: h\nport: 1\nworkers: 99".len();

    // Act
    let actions = actions_at(
        &Yaml,
        &schema,
        text,
        offset,
        &server_diagnostics(text),
        Some(&[CodeActionKind::QUICKFIX]),
    );

    // Assert
    assert_eq!(actions.len(), 1, "the requested quickfix kind passes");
}

/// A spec whose defaults violate their own constraints, which the derive
/// permits.
#[derive(confval::Spec)]
struct BadDefaultsSpec {
    /// A keyword default outside its set.
    #[confval(default = "loud".to_string(), keywords = fixture::LimitMode)]
    mode: Located<String>,
    /// A range default outside its bounds.
    #[confval(default = 9999, range = BAD_WORKERS)]
    workers: Located<i64>,
    /// A length default outside its bound.
    #[confval(default = "toolong".to_string(), length = BAD_NAME)]
    name: Located<String>,
    /// A format default that does not parse.
    #[confval(default = "nope".to_string(), format = confval::Ipv4)]
    bind: Located<String>,
}

range_constraint!(BAD_WORKERS, i64, min: 1, max: 512);
length_constraint!(BAD_NAME, min: 1, max: 3);

impl Validate for BadDefaultsSpec {
    fn validate(&self, _report: &mut Report) {}
}

#[test]
fn a_metacharacter_default_at_a_value_position_stays_literal() {
    // Arrange
    // The value item's text is a literal, not a snippet, so a `$`-bearing
    // default reaches a snippet client unexpanded and a plain client whole.
    let schema = BadgeSpec::schema();
    let text = "home: ";
    let offset = text.len();

    // Act
    let with = complete(&Yaml, &schema, text, offset, true, true);
    let without = complete(&Yaml, &schema, text, offset, false, true);

    // Assert
    let snippet_item = with.first().expect("the default item");
    assert_eq!(
        snippet_item.insert_text_format, None,
        "a literal, not a snippet"
    );
    assert_eq!(inserted(snippet_item), "\"${HOME}/x\"");
    assert_eq!(
        inserted(without.first().expect("the default item")),
        "\"${HOME}/x\""
    );
}

#[test]
fn a_default_violating_its_own_constraint_offers_no_fix() {
    // Arrange
    let keyword_text = "mode: \"bogus\"\nworkers: 1\n";
    let range_text = "mode: \"enforce\"\nworkers: 70000\n";
    let length_text = "mode: \"enforce\"\nworkers: 1\nname: \"abcdef\"\n";
    let format_text = "mode: \"enforce\"\nworkers: 1\nbind: \"also-bad\"\n";
    let schema = BadDefaultsSpec::schema();
    let keyword_diagnostics = {
        full_diagnostics::<BadDefaultsSpec, _>(&Yaml, &schema, keyword_text, &doc_uri(), ENCODING)
    };
    let range_diagnostics = {
        full_diagnostics::<BadDefaultsSpec, _>(&Yaml, &schema, range_text, &doc_uri(), ENCODING)
    };
    let length_diagnostics = {
        full_diagnostics::<BadDefaultsSpec, _>(&Yaml, &schema, length_text, &doc_uri(), ENCODING)
    };
    let format_diagnostics = {
        full_diagnostics::<BadDefaultsSpec, _>(&Yaml, &schema, format_text, &doc_uri(), ENCODING)
    };

    // Act
    let keyword_actions = actions_at(
        &Yaml,
        &schema,
        keyword_text,
        keyword_text.find("bogus").unwrap(),
        &keyword_diagnostics,
        None,
    );
    let range_actions = actions_at(
        &Yaml,
        &schema,
        range_text,
        range_text.find("70000").unwrap(),
        &range_diagnostics,
        None,
    );
    let length_actions = actions_at(
        &Yaml,
        &schema,
        length_text,
        length_text.find("abcdef").unwrap(),
        &length_diagnostics,
        None,
    );
    let format_actions = actions_at(
        &Yaml,
        &schema,
        format_text,
        format_text.find("also-bad").unwrap(),
        &format_diagnostics,
        None,
    );

    // Assert
    assert!(
        keyword_actions.is_empty(),
        "a default outside the keyword set fixes nothing: {keyword_actions:?}"
    );
    assert!(
        range_actions.is_empty(),
        "a default outside the range fixes nothing: {range_actions:?}"
    );
    assert!(
        length_actions.is_empty(),
        "a default outside the length bound fixes nothing: {length_actions:?}"
    );
    assert!(
        format_actions.is_empty(),
        "a default that does not parse fixes nothing: {format_actions:?}"
    );
}

#[test]
fn a_diagnostic_beyond_the_buffer_offers_no_fix() {
    // Arrange
    let text = "hostname: h\nport: 1\nworkers: 9999";
    let schema = ServerSpec::schema();
    let stale = vec![lsp_types::Diagnostic {
        range: lsp_types::Range {
            start: lsp_types::Position {
                line: 40,
                character: 0,
            },
            end: lsp_types::Position {
                line: 41,
                character: 0,
            },
        },
        message: "stale".to_string(),
        ..lsp_types::Diagnostic::default()
    }];

    // Act
    let actions = actions_at(
        &Yaml,
        &schema,
        text,
        text.find("9999").unwrap(),
        &stale,
        None,
    );

    // Assert
    assert!(
        actions.is_empty(),
        "an out-of-buffer range is not contained"
    );
}

/// A labeled block plus a defaulted reference to it, for the reference rows.
#[derive(confval::Spec)]
struct RefDefaultSpec {
    /// The labeled upstreams.
    #[confval(nested)]
    upstream: Vec<Located<RefUpstream>>,
    /// A defaulted reference, whose value position offers labels only.
    #[confval(default = "api".to_string(), references = upstream)]
    target: Located<String>,
}

/// One labeled upstream of the reference fixture.
#[derive(confval::Spec)]
struct RefUpstream {
    /// The label.
    #[confval(label)]
    name: Located<String>,
}

impl Validate for RefDefaultSpec {
    fn validate(&self, _report: &mut Report) {}
}

impl Validate for RefUpstream {
    fn validate(&self, _report: &mut Report) {}
}

#[test]
fn the_remaining_constraint_shapes_answer_per_the_table() {
    // Arrange
    // An unconstrained defaulted scalar offers its default, a defaulted
    // reference offers the labels only, and a keyword default outside its set
    // preselects nothing.
    let unconstrained_text = "tls: \n";
    let reference_text = "upstream:\n  - name: web\ntarget: \n";
    let keyword_text = "mode: \nworkers: 1\n";

    // Act
    let unconstrained = complete(
        &Yaml,
        &ServerSpec::schema(),
        unconstrained_text,
        "tls: ".len(),
        false,
        true,
    );
    let reference = complete(
        &Yaml,
        &RefDefaultSpec::schema(),
        reference_text,
        reference_text.find("target: ").unwrap() + "target: ".len(),
        false,
        true,
    );
    let keyword = complete(
        &Yaml,
        &BadDefaultsSpec::schema(),
        keyword_text,
        "mode: ".len(),
        false,
        true,
    );

    // Assert
    assert_eq!(
        labels_of(&unconstrained),
        vec!["true", "false"],
        "an empty boolean offers the closed set"
    );
    assert_eq!(unconstrained[1].preselect, Some(true));
    assert_eq!(
        labels_of(&reference),
        vec!["web"],
        "labels only, never the default"
    );
    assert!(reference[0].preselect.is_none());
    assert_eq!(labels_of(&keyword), vec!["enforce", "log", "off"]);
    assert!(
        keyword.iter().all(|item| item.preselect.is_none()),
        "a default outside the set preselects nothing"
    );
}

/// The labels of a set of completion items.
fn labels_of(items: &[CompletionItem]) -> Vec<String> {
    items.iter().map(|item| item.label.clone()).collect()
}

#[test]
fn a_defaulted_reference_offers_no_fix_and_an_empty_kind_admits_one() {
    // Arrange
    let reference_text = "upstream:\n  - name: web\ntarget: \"nope\"\n";
    let reference_schema = RefDefaultSpec::schema();
    let reference_diagnostics = full_diagnostics::<RefDefaultSpec, _>(
        &Yaml,
        &reference_schema,
        reference_text,
        &doc_uri(),
        ENCODING,
    );
    let fix_text = "hostname: h\nport: 1\nworkers: 9999\n";
    let fix_schema = ServerSpec::schema();
    let fix_diagnostics = server_diagnostics(fix_text);
    let empty_kind = [CodeActionKind::from("".to_string())];

    // Act
    let reference_actions = actions_at(
        &Yaml,
        &reference_schema,
        reference_text,
        reference_text.find("nope").unwrap(),
        &reference_diagnostics,
        None,
    );
    let admitted = actions_at(
        &Yaml,
        &fix_schema,
        fix_text,
        fix_text.find("9999").unwrap(),
        &fix_diagnostics,
        Some(&empty_kind),
    );

    // Assert
    assert!(
        reference_actions.is_empty(),
        "a reference's default is not a value to reset to"
    );
    assert_eq!(admitted.len(), 1, "the empty kind admits everything");
}

#[test]
fn hover_prints_a_boolean_default() {
    // Arrange
    // The half-typed `tls` does not parse, so the state is unknown and the
    // default line renders.
    let text = "port: 1\ntls";
    let schema = ServerSpec::schema();

    // Act
    let markdown = hover_markdown(&Yaml, &schema, text, text.len());

    // Assert
    assert!(markdown.contains("Defaults to false."), "{markdown}");
}

/// A spec with an unwieldy string default, for the title elision.
#[derive(confval::Spec)]
struct LongDefaultSpec {
    /// A sixty-character default.
    #[confval(default = "012345678901234567890123456789012345678901234567890123456789".to_string())]
    banner: Located<String>,
}

impl Validate for LongDefaultSpec {
    fn validate(&self, _report: &mut Report) {}
}

#[test]
fn a_stale_character_position_past_the_line_offers_no_fix() {
    // Arrange
    // The diagnostic's line exists, and its characters are past the line's
    // content, so the clamped offsets must not read as containment.
    let text = "hostname: h\nport: 1\nworkers: 9999\n";
    let schema = ServerSpec::schema();
    let stale = vec![lsp_types::Diagnostic {
        range: lsp_types::Range {
            start: lsp_types::Position {
                line: 2,
                character: 50,
            },
            end: lsp_types::Position {
                line: 2,
                character: 60,
            },
        },
        message: "stale".to_string(),
        ..lsp_types::Diagnostic::default()
    }];

    // Act
    let actions = actions_at(
        &Yaml,
        &schema,
        text,
        text.find("9999").unwrap(),
        &stale,
        None,
    );

    // Assert
    assert!(actions.is_empty(), "a clamped character is not containment");
}

#[test]
fn a_long_default_elides_in_the_title_and_stays_whole_in_the_edit() {
    // Arrange
    let text = "banner: 123\n";
    let schema = LongDefaultSpec::schema();
    let found = full_diagnostics::<LongDefaultSpec, _>(&Yaml, &schema, text, &doc_uri(), ENCODING);

    // Act
    let actions = actions_at(
        &Yaml,
        &schema,
        text,
        text.find("123").unwrap(),
        &found,
        None,
    );

    // Assert
    let CodeActionOrCommand::CodeAction(action) = actions.first().expect("one fix") else {
        panic!("a code action");
    };
    assert!(
        action.title.ends_with("..."),
        "the title elides: {}",
        action.title
    );
    let edits: Vec<lsp_types::TextEdit> = action
        .edit
        .as_ref()
        .and_then(|edit| edit.changes.as_ref())
        .map(|changes| changes.values().flatten().cloned().collect())
        .unwrap_or_default();
    assert!(
        edits[0].new_text.len() > 50,
        "the edit keeps the whole default"
    );
}

/// A spec with one undefaulted boolean, for the closed-set completion.
#[derive(confval::Spec)]
struct PlainBoolSpec {
    /// A boolean with no default.
    strict: Located<bool>,
}

impl Validate for PlainBoolSpec {
    fn validate(&self, _report: &mut Report) {}
}

#[test]
fn a_set_bool_value_offers_the_other_literal() {
    // Arrange
    // A written boolean completes to the value it could change to, so `true`
    // offers `false` alone and `false` offers `true` alone.
    let on_true = "hostname: h\nport: 1\ntls: true\n";
    let on_false = "hostname: h\nport: 1\ntls: false\n";
    let schema = ServerSpec::schema();

    // Act
    let from_true = complete(
        &Yaml,
        &schema,
        on_true,
        on_true.find("true").unwrap() + 1,
        false,
        true,
    );
    let from_false = complete(
        &Yaml,
        &schema,
        on_false,
        on_false.find("false").unwrap() + 1,
        false,
        true,
    );

    // Assert
    assert_eq!(labels_of(&from_true), vec!["false"]);
    assert_eq!(labels_of(&from_false), vec!["true"]);
}

#[test]
fn an_empty_bool_value_offers_both_with_the_default_preselected() {
    // Arrange
    let text = "hostname: h\nport: 1\ntls: \n";
    let offset = text.find("tls: ").unwrap() + "tls: ".len();
    let schema = ServerSpec::schema();

    // Act
    let items = complete(&Yaml, &schema, text, offset, false, true);

    // Assert
    assert_eq!(labels_of(&items), vec!["true", "false"]);
    let selected: Vec<&str> = items
        .iter()
        .filter(|item| item.preselect == Some(true))
        .map(|item| item.label.as_str())
        .collect();
    assert_eq!(selected, vec!["false"], "the default preselects");
}

#[test]
fn an_undefaulted_bool_offers_both_literals_with_no_preselect() {
    // Arrange
    let text = "strict: \n";
    let schema = PlainBoolSpec::schema();

    // Act
    let items = complete(&Yaml, &schema, text, "strict: ".len(), false, true);

    // Assert
    assert_eq!(labels_of(&items), vec!["true", "false"]);
    assert!(items.iter().all(|item| item.preselect.is_none()));
}

#[test]
fn json_bool_completion_replaces_the_whole_literal() {
    // Arrange
    // The cursor is mid-literal, on the `r` of `true`. The replace range
    // covers the whole literal, so accepting `false` cannot splice the two.
    let text = "{ \"hostname\": \"h\", \"port\": 1, \"tls\": true }";
    let offset = text.find("true").unwrap() + 2;
    let schema = ServerSpec::schema();

    // Act
    let items = complete(&Json, &schema, text, offset, false, false);

    // Assert
    assert_eq!(labels_of(&items), vec!["false"]);
    let edit = match &items[0].text_edit {
        Some(CompletionTextEdit::Edit(edit)) => edit,
        other => panic!("a replace edit, got {other:?}"),
    };
    let start = text.find("true").unwrap();
    assert_eq!(
        (
            edit.range.start.character as usize,
            edit.range.end.character as usize
        ),
        (start, start + "true".len()),
        "the whole literal is replaced"
    );
    assert_eq!(edit.new_text, "false");
}

#[test]
fn kdl_bool_literals_complete_in_their_hash_form() {
    // Arrange
    let text = "hostname \"h\"\nport 1\ntls #true\n";
    let offset = text.find("#true").unwrap() + 2;
    let schema = ServerSpec::schema();

    // Act
    let items = complete(&Kdl, &schema, text, offset, false, false);

    // Assert
    assert_eq!(labels_of(&items), vec!["false"], "only the other literal");
    assert_eq!(inserted(&items[0]), "#false");
}

#[test]
fn json_body_completion_offers_only_the_schema_fields() {
    // Arrange
    // The server never offers bare `true`, `false`, or `null` at a body
    // position. An editor's own JSON assistance may add them beside the
    // server's items, which is outside the protocol.
    let text = "{ \"hostname\": \"h\", ";
    let schema = ServerSpec::schema();

    // Act
    let items = complete(&Json, &schema, text, text.len(), false, false);

    // Assert
    let labels = labels_of(&items);
    assert!(labels.contains(&"port".to_string()));
    for keyword in ["true", "false", "null"] {
        assert!(
            !labels.contains(&keyword.to_string()),
            "no bare literal at a body position: {labels:?}"
        );
    }
}
