//! Rename and prepare rename against the fixtures: the label and every
//! reference change in one edit, inside their quotes, in every format.

mod fixture;

use std::str::FromStr;

use lsp_types::{PrepareRenameResponse, Uri, WorkspaceEdit};

use confval::schema::{Schema, ToSchema};
use confval_lsp::handlers::{prepare_rename, rename};
use confval_lsp::{Frontend, Hcl, Json, Kdl, LineIndex, PositionEncoding, Toml, Yaml};

use fixture::{GatewaySpec, MeshSpec};

const UTF8: PositionEncoding = PositionEncoding::Utf8;

const GATEWAY_HCL: &str = "upstream \"api\" {\n  host = \"h\"\n  port = 1\n}\nroutes {\n  prefix = \"/a\"\n  upstream = \"api\"\n}\nroutes {\n  prefix = \"/b\"\n  upstream = \"api\"\n}\n";
const GATEWAY_KDL: &str = "upstream \"api\" {\n  host \"h\"\n  port 1\n}\nroutes {\n  prefix \"/a\"\n  upstream \"api\"\n}\n";
const GATEWAY_TOML: &str = "[[upstream]]\nname = \"api\"\nhost = \"h\"\nport = 1\n\n[[routes]]\nprefix = \"/a\"\nupstream = \"api\"\n";
const GATEWAY_JSON: &str = "{\n  \"upstream\": [{\"name\": \"api\", \"host\": \"h\", \"port\": 1}],\n  \"routes\": [{\"prefix\": \"/a\", \"upstream\": \"api\"}]\n}\n";
const GATEWAY_YAML_QUOTED: &str = "upstream:\n  - name: \"api\"  # the api\n    host: h\n    port: 1\nroutes:\n  - prefix: /a\n    upstream: \"api\"\n";
const GATEWAY_YAML_BARE: &str = "upstream:\n  - name: api\n    host: h\n    port: 1\nroutes:\n  - prefix: /a\n    upstream: api\n";

fn doc_uri() -> Uri {
    match Uri::from_str("file:///doc") {
        Ok(uri) => uri,
        Err(_) => panic!("a valid uri"),
    }
}

/// Resolves a cursor and runs the rename handler.
fn rename_at<F: Frontend>(
    frontend: &F,
    schema: &Schema,
    text: &str,
    offset: usize,
    new_name: &str,
    encoding: PositionEncoding,
) -> Result<Option<WorkspaceEdit>, String> {
    let tree = frontend.parse_tree(text);
    let context = frontend.resolve(tree.as_ref(), text, offset);
    let index = LineIndex::new(text);
    rename(
        schema,
        &context,
        &doc_uri(),
        text,
        &index,
        encoding,
        new_name,
    )
}

/// Resolves a cursor and runs the prepare-rename handler, answering the text
/// the range covers.
fn prepare_at<F: Frontend>(
    frontend: &F,
    schema: &Schema,
    text: &str,
    offset: usize,
) -> Option<String> {
    let tree = frontend.parse_tree(text);
    let context = frontend.resolve(tree.as_ref(), text, offset);
    let index = LineIndex::new(text);
    let response = prepare_rename(schema, &context, text, &index, UTF8)?;
    let PrepareRenameResponse::Range(range) = response else {
        panic!("a plain range");
    };
    let start = index.offset_of(text, range.start, UTF8);
    let end = index.offset_of(text, range.end, UTF8);
    Some(text[start..end].to_string())
}

/// Applies a workspace edit to the text, last edit first so earlier offsets
/// stay valid.
fn apply(text: &str, edit: &WorkspaceEdit, encoding: PositionEncoding) -> String {
    let index = LineIndex::new(text);
    let Some(changes) = edit.changes.as_ref() else {
        panic!("a workspace edit with changes");
    };
    let mut edits: Vec<(usize, usize, String)> = changes
        .values()
        .flatten()
        .map(|edit| {
            (
                index.offset_of(text, edit.range.start, encoding),
                index.offset_of(text, edit.range.end, encoding),
                edit.new_text.clone(),
            )
        })
        .collect();
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.0));
    let mut out = text.to_string();
    for (start, end, new_text) in edits {
        out.replace_range(start..end, &new_text);
    }
    out
}

/// Renames from the cursor and answers the edited text.
fn renamed<F: Frontend>(
    frontend: &F,
    schema: &Schema,
    text: &str,
    offset: usize,
    new_name: &str,
    encoding: PositionEncoding,
) -> String {
    match rename_at(frontend, schema, text, offset, new_name, encoding) {
        Ok(Some(edit)) => apply(text, &edit, encoding),
        Ok(None) => panic!("a renameable site"),
        Err(message) => panic!("an accepted name: {message}"),
    }
}

/// The line the prepare range starts on, for a cursor at `offset`.
fn prepare_line_at<F: Frontend>(frontend: &F, schema: &Schema, text: &str, offset: usize) -> u32 {
    let tree = frontend.parse_tree(text);
    let context = frontend.resolve(tree.as_ref(), text, offset);
    let index = LineIndex::new(text);
    match prepare_rename(schema, &context, text, &index, UTF8) {
        Some(PrepareRenameResponse::Range(range)) => range.start.line,
        _ => panic!("a plain range"),
    }
}

/// Renames a quoted `api` label from the label and from the last reference,
/// and answers the two edited texts and the two prepare ranges.
fn quoted_both_ways<F: Frontend>(frontend: &F, text: &str) -> ([String; 2], [Option<String>; 2]) {
    let schema = GatewaySpec::schema();
    let (Some(label), Some(reference)) = (text.find("\"api\""), text.rfind("\"api\"")) else {
        panic!("a quoted api label and reference");
    };
    let (label, reference) = (label + 1, reference + 1);
    (
        [
            renamed(frontend, &schema, text, label, "backend", UTF8),
            renamed(frontend, &schema, text, reference, "backend", UTF8),
        ],
        [
            prepare_at(frontend, &schema, text, label),
            prepare_at(frontend, &schema, text, reference),
        ],
    )
}

#[test]
fn hcl_rename_from_the_label_edits_the_label_and_every_reference() {
    // Arrange
    let offset = GATEWAY_HCL.find("\"api\"").unwrap() + 1;

    // Act
    let edited = renamed(
        &Hcl,
        &GatewaySpec::schema(),
        GATEWAY_HCL,
        offset,
        "backend",
        UTF8,
    );

    // Assert
    assert_eq!(edited, GATEWAY_HCL.replace("\"api\"", "\"backend\""));
    assert_eq!(
        prepare_at(&Hcl, &GatewaySpec::schema(), GATEWAY_HCL, offset),
        Some("api".to_string())
    );
}

#[test]
fn hcl_rename_from_a_reference_edits_the_same_set() {
    // Arrange
    let offset = GATEWAY_HCL.rfind("\"api\"").unwrap() + 1;

    // Act
    let edited = renamed(
        &Hcl,
        &GatewaySpec::schema(),
        GATEWAY_HCL,
        offset,
        "backend",
        UTF8,
    );

    // Assert
    assert_eq!(edited, GATEWAY_HCL.replace("\"api\"", "\"backend\""));
    assert_eq!(
        prepare_at(&Hcl, &GatewaySpec::schema(), GATEWAY_HCL, offset),
        Some("api".to_string())
    );
    assert_eq!(
        prepare_line_at(&Hcl, &GatewaySpec::schema(), GATEWAY_HCL, offset),
        10
    );
}

#[test]
fn every_quoted_format_renames_from_the_label_and_from_a_reference() {
    // Arrange
    let texts = [GATEWAY_KDL, GATEWAY_TOML, GATEWAY_JSON, GATEWAY_YAML_QUOTED];

    // Act
    let results = [
        quoted_both_ways(&Kdl, GATEWAY_KDL),
        quoted_both_ways(&Toml, GATEWAY_TOML),
        quoted_both_ways(&Json, GATEWAY_JSON),
        quoted_both_ways(&Yaml, GATEWAY_YAML_QUOTED),
    ];

    // Assert
    for (text, (edited, prepared)) in texts.iter().zip(results) {
        let expected = text.replace("\"api\"", "\"backend\"");
        assert_eq!(edited, [expected.clone(), expected]);
        assert_eq!(prepared, [Some("api".to_string()), Some("api".to_string())]);
    }
}

#[test]
fn a_bare_yaml_label_stays_bare() {
    // Arrange
    let schema = GatewaySpec::schema();
    let label = GATEWAY_YAML_BARE.find("name: api").unwrap() + "name: a".len();
    let reference = GATEWAY_YAML_BARE.rfind("upstream: api").unwrap() + "upstream: a".len();

    // Act
    let from_label = renamed(&Yaml, &schema, GATEWAY_YAML_BARE, label, "backend", UTF8);
    let from_reference = renamed(
        &Yaml,
        &schema,
        GATEWAY_YAML_BARE,
        reference,
        "backend",
        UTF8,
    );

    // Assert
    let expected = GATEWAY_YAML_BARE.replace("api", "backend");
    assert_eq!(from_label, expected);
    assert_eq!(from_reference, expected);
    assert_eq!(
        prepare_at(&Yaml, &schema, GATEWAY_YAML_BARE, label),
        Some("api".to_string())
    );
    assert_eq!(
        prepare_at(&Yaml, &schema, GATEWAY_YAML_BARE, reference),
        Some("api".to_string())
    );
}

#[test]
fn single_quoted_labels_rename_inside_their_quotes() {
    // Arrange
    let schema = GatewaySpec::schema();
    let toml = GATEWAY_TOML.replace("name = \"api\"", "name = 'api'");
    let yaml = GATEWAY_YAML_QUOTED.replace("name: \"api\"", "name: 'api'");
    let toml_offset = toml.find("'api'").unwrap() + 1;
    let yaml_offset = yaml.find("'api'").unwrap() + 1;

    // Act
    let toml_out = renamed(&Toml, &schema, &toml, toml_offset, "backend", UTF8);
    let yaml_out = renamed(&Yaml, &schema, &yaml, yaml_offset, "backend", UTF8);

    // Assert
    assert_eq!(
        toml_out,
        toml.replace("'api'", "'backend'")
            .replace("\"api\"", "\"backend\"")
    );
    assert_eq!(
        yaml_out,
        yaml.replace("'api'", "'backend'")
            .replace("\"api\"", "\"backend\"")
    );
    assert_eq!(
        prepare_at(&Toml, &schema, &toml, toml_offset),
        Some("api".to_string())
    );
    assert_eq!(
        prepare_at(&Yaml, &schema, &yaml, yaml_offset),
        Some("api".to_string())
    );
}

#[test]
fn bare_native_labels_rename_and_a_raw_string_label_is_refused() {
    // Arrange
    let hcl = GATEWAY_HCL.replace("upstream \"api\"", "upstream api");
    let kdl = GATEWAY_KDL.replace("upstream \"api\"", "upstream api");
    let raw = GATEWAY_KDL.replace("upstream \"api\"", "upstream #\"api\"#");
    let schema = GatewaySpec::schema();
    let hcl_offset = hcl.find("upstream api").unwrap() + 9;
    let kdl_offset = kdl.find("upstream api").unwrap() + 9;
    let raw_offset = raw.find("#\"api\"#").unwrap() + 2;

    // Act
    let hcl_out = renamed(&Hcl, &schema, &hcl, hcl_offset, "backend", UTF8);
    let kdl_out = renamed(&Kdl, &schema, &kdl, kdl_offset, "backend", UTF8);
    let raw_out = rename_at(&Kdl, &schema, &raw, raw_offset, "backend", UTF8);

    // Assert
    assert_eq!(hcl_out, hcl.replace("api", "backend"));
    assert_eq!(kdl_out, kdl.replace("api", "backend"));
    assert_eq!(
        prepare_at(&Hcl, &schema, &hcl, hcl_offset),
        Some("api".to_string())
    );
    assert_eq!(
        prepare_at(&Kdl, &schema, &kdl, kdl_offset),
        Some("api".to_string())
    );
    assert_eq!(
        raw_out,
        Err(
            "a label or a reference is written as a raw or escaped string and cannot be rewritten"
                .to_string()
        )
    );
    assert_eq!(prepare_at(&Kdl, &schema, &raw, raw_offset), None);
}

#[test]
fn a_shadowing_inner_label_keeps_the_outer_rename_out_of_its_scope() {
    // Arrange
    let text = "pools \"p\" {\n}\nservices {\n  name = \"s\"\n  pools \"p\" {\n  }\n  routes {\n    upstream = \"u\"\n    pool = \"p\"\n  }\n  upstreams \"u\" {\n    port = 1\n  }\n}\n";
    let offset = text.find("\"p\"").unwrap() + 1;

    // Act
    let edited = renamed(&Hcl, &MeshSpec::schema(), text, offset, "q", UTF8);

    // Assert
    assert_eq!(edited, text.replacen("\"p\"", "\"q\"", 1));
}

#[test]
fn nothing_renameable_answers_none() {
    // Arrange
    let schema = GatewaySpec::schema();
    let empty = GATEWAY_HCL.replace("upstream \"api\"", "upstream \"\"");
    let unparsed = "upstream \"api\" {\n  host = \"h\"\nroutes {\n  upstream = \"api\"\n";
    let duplicate = format!("upstream \"api\" {{\n  host = \"x\"\n  port = 9\n}}\n{GATEWAY_HCL}");

    // Act
    let on_name = rename_at(
        &Yaml,
        &schema,
        GATEWAY_YAML_BARE,
        GATEWAY_YAML_BARE.find("name:").unwrap() + 1,
        "x",
        PositionEncoding::Utf8,
    );
    let on_empty = rename_at(
        &Hcl,
        &schema,
        &empty,
        empty.find("\"\"").unwrap() + 1,
        "x",
        PositionEncoding::Utf8,
    );
    let on_unparsed = rename_at(
        &Hcl,
        &schema,
        unparsed,
        unparsed.find("\"api\"").unwrap() + 1,
        "x",
        PositionEncoding::Utf8,
    );
    let on_first = rename_at(
        &Hcl,
        &schema,
        &duplicate,
        duplicate.find("\"api\"").unwrap() + 1,
        "x",
        PositionEncoding::Utf8,
    );
    let on_second = rename_at(
        &Hcl,
        &schema,
        &duplicate,
        duplicate
            .find("upstream \"api\" {\n  host = \"h\"")
            .unwrap()
            + 10,
        "x",
        PositionEncoding::Utf8,
    );
    let on_reference = rename_at(
        &Hcl,
        &schema,
        &duplicate,
        duplicate.rfind("\"api\"").unwrap() + 1,
        "x",
        PositionEncoding::Utf8,
    );
    let prepared = prepare_at(
        &Hcl,
        &schema,
        &duplicate,
        duplicate.rfind("\"api\"").unwrap() + 1,
    );

    // Assert
    assert_eq!(on_name, Ok(None));
    assert_eq!(on_empty, Ok(None));
    assert_eq!(on_unparsed, Ok(None));
    assert_eq!(on_first, Ok(None));
    assert_eq!(on_second, Ok(None));
    assert_eq!(on_reference, Ok(None));
    assert_eq!(prepared, None);
}

#[test]
fn a_refused_name_answers_the_reason() {
    // Arrange
    let schema = GatewaySpec::schema();
    let offset = GATEWAY_HCL.find("\"api\"").unwrap() + 1;
    let bare = GATEWAY_YAML_BARE.find("name: api").unwrap() + "name: a".len();
    let single = GATEWAY_TOML.replace("name = \"api\"", "name = 'api'");
    let single_offset = single.find("'api'").unwrap() + 1;
    let breaks = "a label cannot contain a quote, a backslash, a line break, `${`, or `%{`";
    let class =
        "a bare label takes letters, digits, `_`, and `-` only, starting with a letter or `_`";

    // Act
    let blank = rename_at(&Hcl, &schema, GATEWAY_HCL, offset, "  ", UTF8);
    let quoted = rename_at(&Hcl, &schema, GATEWAY_HCL, offset, "a\"b", UTF8);
    let escaped = rename_at(&Hcl, &schema, GATEWAY_HCL, offset, "a\\b", UTF8);
    let multi = rename_at(&Hcl, &schema, GATEWAY_HCL, offset, "a\nb", UTF8);
    let template = rename_at(&Hcl, &schema, GATEWAY_HCL, offset, "${x}", UTF8);
    let spaced_bare = rename_at(&Yaml, &schema, GATEWAY_YAML_BARE, bare, "my api", UTF8);
    let dotted_bare = rename_at(&Yaml, &schema, GATEWAY_YAML_BARE, bare, "api.v2", UTF8);
    let literal_bare = rename_at(&Yaml, &schema, GATEWAY_YAML_BARE, bare, "true", UTF8);
    let apostrophe = rename_at(&Toml, &schema, &single, single_offset, "it's", UTF8);

    // Assert
    assert_eq!(blank, Err("a label cannot be empty".to_string()));
    assert_eq!(quoted, Err(breaks.to_string()));
    assert_eq!(escaped, Err(breaks.to_string()));
    assert_eq!(multi, Err(breaks.to_string()));
    assert_eq!(template, Err(breaks.to_string()));
    assert_eq!(spaced_bare, Err(class.to_string()));
    assert_eq!(dotted_bare, Err(class.to_string()));
    assert_eq!(
        literal_bare,
        Err("a bare label cannot be the literal word `true`".to_string())
    );
    assert_eq!(
        apostrophe,
        Err("a single-quoted label cannot contain a single quote".to_string())
    );
}

#[test]
fn the_old_name_answers_the_same_edit_set() {
    // Arrange
    let offset = GATEWAY_HCL.find("\"api\"").unwrap() + 1;

    // Act
    let edited = renamed(
        &Hcl,
        &GatewaySpec::schema(),
        GATEWAY_HCL,
        offset,
        "api",
        UTF8,
    );

    // Assert
    assert_eq!(edited, GATEWAY_HCL);
}

#[test]
fn utf16_ranges_land_after_a_non_ascii_value_on_the_same_line() {
    // Arrange
    let text = GATEWAY_JSON.replace("\"/a\"", "\"/é\"").replace(
        "{\"name\": \"api\", \"host\": \"h\"",
        "{\"host\": \"é\", \"name\": \"api\"",
    );
    let offset = text.find("\"api\"").unwrap() + 1;

    // Act
    let edited = renamed(
        &Json,
        &GatewaySpec::schema(),
        &text,
        offset,
        "backend",
        PositionEncoding::Utf16,
    );

    // Assert
    assert_eq!(edited, text.replace("\"api\"", "\"backend\""));
}
