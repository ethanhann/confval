//! The rendered-default consumers: the hover value, the pre-filled insert, the
//! preselected value item, and the reset-to-default quick fix.

mod fixture;

use std::str::FromStr;

use lsp_types::{
    CodeActionKind, CodeActionOrCommand, CompletionItem, CompletionTextEdit, HoverContents,
    InsertTextFormat, Uri,
};

use confval::prelude::{Located, Report, Validate};
use confval::schema::ToSchema;
use confval_lsp::handlers::{Cx, code_action, completion, diagnostics, hover};
use confval_lsp::{Frontend, Hcl, Json, Kdl, LineIndex, PositionEncoding, Toml, Yaml};

use fixture::{LimitsSpec, ServerSpec};

const ENCODING: PositionEncoding = PositionEncoding::Utf8;

/// A spec whose string default carries snippet metacharacters.
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
        snippets,
        preselect,
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
    let found = hover(schema, tree.as_ref(), &context, text, &index, ENCODING).expect("a hover");
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
    let uri = Uri::from_str("file:///doc").unwrap();
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
    let uri = Uri::from_str("file:///doc").unwrap();
    diagnostics::<ServerSpec, Yaml>(&Yaml, &ServerSpec::schema(), text, &uri, ENCODING)
}

#[test]
fn hover_prints_the_rendered_default_for_the_three_states() {
    // Arrange
    let schema = ServerSpec::schema();
    let set = "hostname: h\nport: 1\nworkers: 2\n";
    let unset = "hostname: h\nport: 1\nlimits:\n  mode\n";
    let uncarried = "hostname: h\nport: 1\nallow:\n  - a\n";

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
    assert!(set_markdown.contains("Defaults to 4."), "{set_markdown}");
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
    // `mode` carries keywords, so the default's item is preselected with no
    // duplicate. `workers` carries a range, so the rendered default is the one
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

#[test]
fn the_rendered_default_round_trips_through_every_frontend() {
    // Arrange
    // Completing `workers` on an empty document and parsing the result back
    // yields the default value, so each frontend's literal form is one its
    // parser reads.
    let schema = ServerSpec::schema();

    // Act, Assert
    fn round_trip<F: Frontend>(
        frontend: &F,
        schema: &confval::schema::Schema,
        wrap: impl Fn(&str) -> String,
    ) {
        let items = complete(frontend, schema, "", 0, false, false);
        let workers = inserted(
            items
                .iter()
                .find(|i| i.label == "workers")
                .expect("workers"),
        );
        let tls = inserted(items.iter().find(|i| i.label == "tls").expect("tls"));
        let document = wrap(&format!("{workers}\n{tls}"));
        let tree = frontend
            .parse_tree(&document)
            .unwrap_or_else(|| panic!("the completed document parses: {document:?}"));
        assert!(tree.has("workers") && tree.has("tls"), "{document:?}");
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
    let changes = action.edit.as_ref().unwrap().changes.as_ref().unwrap();
    let edit = &changes.values().next().unwrap()[0];
    assert_eq!(edit.new_text, "4");
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
fn the_negative_cases_offer_no_fix() {
    // Arrange
    let schema = ServerSpec::schema();
    // `port` carries no default, so its range violation offers nothing.
    let no_default = "hostname: h\nport: 99999\n";
    // The missing-required diagnostic anchors at the enclosing span, so it
    // never lights a defaulted value elsewhere.
    let enclosing = "port: 1\nworkers: 2\n";
    let no_parse = "workers: 9999\nbad: [\n";

    // Act
    let no_default_actions = actions_at(
        &Yaml,
        &schema,
        no_default,
        no_default.find("99999").unwrap(),
        &server_diagnostics(no_default),
        None,
    );
    let enclosing_actions = actions_at(
        &Yaml,
        &schema,
        enclosing,
        enclosing.find('2').unwrap(),
        &server_diagnostics(enclosing),
        None,
    );
    let body_actions = actions_at(
        &Yaml,
        &schema,
        "hostname: h\nport: 1\nworkers: 9999\n",
        1,
        &server_diagnostics("hostname: h\nport: 1\nworkers: 9999\n"),
        None,
    );
    let empty_context = actions_at(
        &Yaml,
        &schema,
        "hostname: h\nport: 1\nworkers: 9999\n",
        "hostname: h\nport: 1\nworkers: 99".len(),
        &[],
        None,
    );
    let no_parse_actions = actions_at(
        &Yaml,
        &schema,
        no_parse,
        no_parse.find("9999").unwrap(),
        &server_diagnostics(no_parse),
        None,
    );
    let filtered = actions_at(
        &Yaml,
        &schema,
        "hostname: h\nport: 1\nworkers: 9999\n",
        "hostname: h\nport: 1\nworkers: 99".len(),
        &server_diagnostics("hostname: h\nport: 1\nworkers: 9999\n"),
        Some(&[CodeActionKind::REFACTOR]),
    );

    // Assert
    assert!(no_default_actions.is_empty(), "no default, no fix");
    assert!(
        enclosing_actions.is_empty(),
        "an enclosing-span diagnostic does not qualify"
    );
    assert!(body_actions.is_empty(), "a body position does not qualify");
    assert!(empty_context.is_empty(), "an empty context yields nothing");
    assert!(no_parse_actions.is_empty(), "the edit needs a parsed span");
    assert!(filtered.is_empty(), "the kind filter is honored");
}
