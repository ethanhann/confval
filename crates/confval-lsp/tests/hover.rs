//! The hover handler against the fixtures: a field renders with its type,
//! constraint, doc comment, and set-versus-defaulted state, repeated blocks
//! read the state from the cursor's own instance, and a reference value names
//! its target block and reports whether it resolves.

mod fixture;
mod support;

use lsp_types::HoverContents;

use confval::schema::ToSchema;
use confval_lsp::handlers::{Cx, hover};
use confval_lsp::{Hcl, Json, Kdl, LineIndex, Toml, Yaml};

use fixture::ServerSpec;
use support::{ENCODING, MESH_YAML, at, at_with, gateway_hover};

/// The Markdown body of a hover.
fn markdown(hover: lsp_types::Hover) -> String {
    match hover.contents {
        HoverContents::Markup(markup) => markup.value,
        other => panic!("expected markup hover, got: {other:?}"),
    }
}

#[test]
fn hover_renders_a_set_field_with_its_type_and_constraint() {
    // Arrange
    let text = "port = 8080\n";
    let offset = text.find("port").unwrap() + 1;
    let (tree, context) = at(text, offset);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let hover = hover(
        &Cx {
            schema: &schema,
            fields: tree.as_ref(),
            ctx: &context,
            text,
        },
        &index,
        ENCODING,
    );

    // Assert
    let value = markdown(hover.expect("a hover for port"));
    assert!(value.contains("integer"), "got: {value}");
    assert!(
        value.contains("The TCP port the server listens on"),
        "the doc comment renders: {value}"
    );
    assert!(value.contains("Between 1 and 65535"), "got: {value}");
    assert!(value.contains("Set by the configuration."), "got: {value}");
}

#[test]
fn hover_omits_the_state_when_the_buffer_does_not_parse() {
    // Arrange
    // A half-typed name does not parse, so the set-versus-defaulted state is
    // unknown. The type and default flag still render, but the state line is
    // omitted rather than guessed as "not set".
    let text = "workers";
    let (tree, context) = at(text, text.len());
    assert!(tree.is_none(), "the buffer does not parse");
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let hover = hover(
        &Cx {
            schema: &schema,
            fields: tree.as_ref(),
            ctx: &context,
            text,
        },
        &index,
        ENCODING,
    );

    // Assert
    let value = markdown(hover.expect("a hover for workers"));
    assert!(value.contains("Defaults to 4."), "got: {value}");
    assert!(!value.contains("Not set"), "the state is omitted: {value}");
    assert!(!value.contains("Set by the configuration"), "{value}");
}

#[test]
fn hover_states_a_declared_but_unset_field_is_defaulted() {
    // Arrange
    // The buffer parses. `workers` appears only in a comment, so it is declared
    // by the schema but absent from the parse, and hover reads it as defaulted.
    let text = "# workers\nport = 8080\n";
    let offset = text.find("workers").unwrap() + 1;
    let (tree, context) = at(text, offset);
    assert!(tree.is_some(), "the buffer parses");
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let hover = hover(
        &Cx {
            schema: &schema,
            fields: tree.as_ref(),
            ctx: &context,
            text,
        },
        &index,
        ENCODING,
    );

    // Assert
    let value = markdown(hover.expect("a hover for workers"));
    assert!(value.contains("Not set. Uses its default."), "got: {value}");
}

#[test]
fn hover_on_a_value_renders_its_field() {
    // Arrange
    let text = "port = 8080\n";
    let offset = text.find("8080").unwrap() + 1;
    let (tree, context) = at(text, offset);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let hover = hover(
        &Cx {
            schema: &schema,
            fields: tree.as_ref(),
            ctx: &context,
            text,
        },
        &index,
        ENCODING,
    );

    // Assert
    let value = markdown(hover.expect("a hover on the value"));
    assert!(value.contains("integer"), "got: {value}");
    assert!(value.contains("Set by the configuration."), "got: {value}");
}

#[test]
fn yaml_hover_renders_the_field_under_the_cursor() {
    // Arrange
    let text = "port: 8080\n";
    let offset = text.find("port").unwrap() + 1;
    let (tree, context) = at_with(&Yaml, text, offset);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let rendered = hover(
        &Cx {
            schema: &schema,
            fields: tree.as_ref(),
            ctx: &context,
            text,
        },
        &index,
        ENCODING,
    );

    // Assert
    let value = markdown(rendered.expect("a hover for port"));
    assert!(value.contains("integer"), "got: {value}");
    assert!(value.contains("Between 1 and 65535"), "got: {value}");
}

#[test]
fn json_hover_renders_the_field_under_the_cursor() {
    // Arrange
    let text = "{ \"port\": 8080 }\n";
    let offset = text.find("port").unwrap() + 1;
    let (tree, context) = at_with(&Json, text, offset);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let rendered = hover(
        &Cx {
            schema: &schema,
            fields: tree.as_ref(),
            ctx: &context,
            text,
        },
        &index,
        ENCODING,
    );

    // Assert
    let value = markdown(rendered.expect("a hover for port"));
    assert!(value.contains("integer"), "got: {value}");
    assert!(value.contains("Between 1 and 65535"), "got: {value}");
}

#[test]
fn hover_reads_the_state_from_the_cursors_instance() {
    // Arrange
    // Only the second upstream sets port. Hover on port in the second instance
    // reports it set. Reading the first instance would report it unset.
    let hcl = "upstream \"a\" {\n  host = \"h\"\n}\nupstream \"b\" {\n  host = \"h2\"\n  port = 8080\n}\n";
    let kdl =
        "upstream \"a\" {\n  host \"h\"\n}\nupstream \"b\" {\n  host \"h2\"\n  port 8080\n}\n";
    let toml = "[[upstream]]\nname = \"a\"\nhost = \"h\"\n\n[[upstream]]\nname = \"b\"\nhost = \"h2\"\nport = 8080\n";
    let json = "{\n  \"upstream\": [\n    { \"name\": \"a\", \"host\": \"h\" },\n    { \"name\": \"b\", \"host\": \"h2\", \"port\": 8080 }\n  ]\n}\n";
    let yaml =
        "upstream:\n  - name: a\n    host: alpha\n  - name: b\n    host: beta\n    port: 8080\n";

    // Act
    let hcl_hover = gateway_hover(&Hcl, hcl, hcl.rfind("port").unwrap() + 1);
    let kdl_hover = gateway_hover(&Kdl, kdl, kdl.rfind("port").unwrap() + 1);
    let toml_hover = gateway_hover(&Toml, toml, toml.rfind("port").unwrap() + 1);
    let json_hover = gateway_hover(&Json, json, json.rfind("port").unwrap() + 1);
    let yaml_hover = gateway_hover(&Yaml, yaml, yaml.rfind("port").unwrap() + 1);

    // Assert
    for (format, markdown) in [
        ("hcl", &hcl_hover),
        ("kdl", &kdl_hover),
        ("toml", &toml_hover),
        ("json", &json_hover),
        ("yaml", &yaml_hover),
    ] {
        assert!(
            markdown.contains("Set by the configuration."),
            "{format} reads port set in the second instance: {markdown:?}"
        );
    }
}

#[test]
fn hover_on_a_reference_value_states_the_target_and_resolution() {
    // Arrange
    // A resolved reference names its target and says it resolves. An undefined
    // reference names the target and says it does not.
    let resolved = "upstream \"api\" {\n  host = \"h\"\n  port = 1\n}\nroutes {\n  prefix = \"/a\"\n  upstream = \"api\"\n}\n";
    let resolved_off = resolved.rfind("upstream = \"api\"").unwrap() + "upstream = \"".len();
    let unresolved = "upstream \"api\" {\n  host = \"h\"\n  port = 1\n}\nroutes {\n  prefix = \"/a\"\n  upstream = \"nope\"\n}\n";
    let unresolved_off = unresolved.rfind("upstream = \"nope\"").unwrap() + "upstream = \"".len();

    // Act
    let resolved_hover = gateway_hover(&Hcl, resolved, resolved_off);
    let unresolved_hover = gateway_hover(&Hcl, unresolved, unresolved_off);

    // Assert
    assert!(
        resolved_hover.contains("References the `upstream` block."),
        "names the target: {resolved_hover:?}"
    );
    assert!(
        resolved_hover.contains("Resolves to a defined label."),
        "reports resolution: {resolved_hover:?}"
    );
    assert!(
        unresolved_hover.contains("Does not resolve to any defined label."),
        "reports a miss: {unresolved_hover:?}"
    );
}

#[test]
fn reference_hover_reports_unknown_resolution_without_a_parse() {
    // Arrange
    // An unterminated value does not parse, so hover names the target but cannot
    // say whether the value resolves.
    let text = "upstream \"api\" {\n  host = \"h\"\n  port = 1\n}\nroutes {\n  prefix = \"/a\"\n  upstream = \"ap";

    // Act
    let markdown = gateway_hover(&Hcl, text, text.len());

    // Assert
    assert!(
        markdown.contains("References the `upstream` block."),
        "names the target: {markdown:?}"
    );
    assert!(
        markdown.contains("Resolution is unknown"),
        "reports unknown resolution: {markdown:?}"
    );
}

#[test]
fn scoped_reference_hover_resolves_within_its_own_scope() {
    // Arrange
    let text = MESH_YAML;
    let offset = text.rfind("ub").unwrap();
    let (tree, context) = at_with(&Yaml, text, offset);
    let index = LineIndex::new(text);
    let schema = fixture::MeshSpec::schema();

    // Act
    let found = hover(
        &Cx {
            schema: &schema,
            fields: tree.as_ref(),
            ctx: &context,
            text,
        },
        &index,
        ENCODING,
    )
    .expect("a hover is produced");

    // Assert
    let markdown = match found.contents {
        HoverContents::Markup(markup) => markup.value,
        _ => panic!("expected a markdown hover"),
    };
    assert!(
        markdown.contains("References the `upstreams` block."),
        "the target line names the block: {markdown}"
    );
    assert!(
        markdown.contains("Resolves to a defined label."),
        "the own-scope label resolves: {markdown}"
    );
}

#[test]
fn yaml_single_quoted_reference_value_hovers_as_resolved() {
    // Arrange
    // The parsed value of `'api'` is `api`, which the pipeline resolves. Hover
    // reads the parsed value from the resolved body, so it agrees.
    let text = "upstream:\n  - name: api\n    host: h\n    port: 1\nroutes:\n  - prefix: /a\n    upstream: 'api'\n";
    let offset = text.rfind("api").unwrap();

    // Act
    let markdown = gateway_hover(&Yaml, text, offset);

    // Assert
    assert!(
        markdown.contains("Resolves to a defined label."),
        "the single-quoted value resolves like diagnostics: {markdown}"
    );
}

#[test]
fn a_parsed_non_string_reference_value_hovers_without_a_resolution_line() {
    // Arrange
    // The reference pass skips a parsed non-string without a report, so hover
    // states the target and no resolution line.
    let text = "upstream:\n  - name: api\n    host: h\n    port: 1\nroutes:\n  - prefix: /a\n    upstream: 123\n";
    let offset = text.rfind("123").unwrap() + 1;

    // Act
    let markdown = gateway_hover(&Yaml, text, offset);

    // Assert
    assert!(
        markdown.contains("References the `upstream` block."),
        "the target line stays: {markdown}"
    );
    assert!(
        !markdown.to_lowercase().contains("resolve"),
        "no resolution claim for a value the pass skips: {markdown}"
    );
}

#[test]
fn hover_on_a_reference_field_name_states_the_target_block() {
    // Arrange
    // The field-name hover renders the constraint line, so a reference field
    // names its target rather than appending an empty section.
    let text = "upstream:\n  - name: api\n    host: h\n    port: 1\nroutes:\n  - prefix: /a\n    upstream: \"api\"\n";
    let offset = text.rfind("upstream:").unwrap() + 1;

    // Act
    let markdown = gateway_hover(&Yaml, text, offset);

    // Assert
    assert!(
        markdown.contains("**upstream**"),
        "the field hover renders: {markdown}"
    );
    assert!(
        markdown.contains("References the `upstream` block."),
        "the constraint line names the target: {markdown}"
    );
}

#[test]
fn hover_on_a_constrained_list_names_the_element_set() {
    // Arrange
    let text = "modes = [\"enforce\"]\n";
    let offset = text.find("modes").expect("the field is present") + 1;
    let (tree, context) = at(text, offset);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let hover = hover(
        &Cx {
            schema: &schema,
            fields: tree.as_ref(),
            ctx: &context,
            text,
        },
        &index,
        ENCODING,
    );

    // Assert
    // The set describes one element, and the label reads the same as it does on
    // a scalar keyword field, so an operator learns the vocabulary from either.
    let body = markdown(hover.expect("a hover is produced"));
    assert!(body.contains("string list"), "body: {body}");
    assert!(body.contains("One of: enforce, log, off."), "body: {body}");
}

#[test]
fn hover_on_an_unconstrained_list_names_no_set() {
    // Arrange
    let text = "allow = [\"10.0.0.0/8\"]\n";
    let offset = text.find("allow").expect("the field is present") + 1;
    let (tree, context) = at(text, offset);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let hover = hover(
        &Cx {
            schema: &schema,
            fields: tree.as_ref(),
            ctx: &context,
            text,
        },
        &index,
        ENCODING,
    );

    // Assert
    let body = markdown(hover.expect("a hover is produced"));
    assert!(body.contains("string list"), "body: {body}");
    assert!(!body.contains("One of:"), "body: {body}");
}

#[test]
fn hover_on_a_non_empty_field_states_the_rule() {
    // Arrange
    let text = "hostname = \"127.0.0.1\"\n";
    let offset = text.find("hostname").expect("the field is present") + 1;
    let (tree, context) = at(text, offset);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let hover = hover(
        &Cx {
            schema: &schema,
            fields: tree.as_ref(),
            ctx: &context,
            text,
        },
        &index,
        ENCODING,
    );

    // Assert
    let body = markdown(hover.expect("a hover is produced"));
    assert!(body.contains("Must not be empty."), "body: {body}");
    assert!(
        body.contains("The address the server binds"),
        "body: {body}"
    );
}

#[test]
fn hover_on_a_field_without_the_flag_omits_the_rule() {
    // Arrange
    let text = "port = 8080\n";
    let offset = text.find("port").expect("the field is present") + 1;
    let (tree, context) = at(text, offset);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let hover = hover(
        &Cx {
            schema: &schema,
            fields: tree.as_ref(),
            ctx: &context,
            text,
        },
        &index,
        ENCODING,
    );

    // Assert
    let body = markdown(hover.expect("a hover is produced"));
    assert!(!body.contains("Must not be empty."), "body: {body}");
}

/// The hover body for `name` in YAML text, or `None` when no hover is produced.
fn yaml_hover_body(text: &str, name: &str) -> Option<String> {
    let offset = text.find(name)? + 1;
    let (tree, context) = at_with(&Yaml, text, offset);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();
    hover(
        &Cx {
            schema: &schema,
            fields: tree.as_ref(),
            ctx: &context,
            text,
        },
        &index,
        ENCODING,
    )
    .map(markdown)
}

#[test]
fn yaml_hover_on_a_block_sequence_list_renders_its_body() {
    // Arrange
    let text = "modes:\n  - \"enforce\"\nport: 8080\n";

    // Act
    let body = yaml_hover_body(text, "modes").expect("a hover is produced");

    // Assert
    assert!(body.contains("string list"), "body: {body}");
    assert!(body.contains("One of: enforce, log, off."), "body: {body}");
}

#[test]
fn yaml_hover_on_a_scalar_beside_a_list_still_renders() {
    // Arrange
    let text = "modes:\n  - \"enforce\"\nport: 8080\n";

    // Act
    let body = yaml_hover_body(text, "port").expect("a hover is produced");

    // Assert
    assert!(body.contains("integer"), "body: {body}");
}

#[test]
fn yaml_hover_on_a_list_between_other_keys_renders_its_body() {
    // Arrange
    // The sample documents put the list after a scalar and before a nested
    // block, which is the shape the UAT hovered.
    let text = "hostname: \"0.0.0.0\"\nmodes:\n  - \"enforce\"\n  - \"shout\"\nlimits:\n  max_body_mb: 64\n";

    // Act
    let body = yaml_hover_body(text, "modes").expect("a hover is produced");

    // Assert
    assert!(body.contains("string list"), "body: {body}");
    assert!(body.contains("One of: enforce, log, off."), "body: {body}");
}

#[test]
fn yaml_hover_on_a_list_element_names_the_list() {
    // Arrange
    // A YAML element sits on its own line, away from the key, so hovering it is
    // the only way an operator reads the list from that line.
    let text = "modes:\n  - \"enforce\"\n  - \"shout\"\n";

    // Act
    let body = yaml_hover_body(text, "shout").expect("a hover is produced");

    // Assert
    assert!(body.contains("string list"), "body: {body}");
    assert!(body.contains("One of: enforce, log, off."), "body: {body}");
}

#[test]
fn yaml_hover_on_a_list_element_reports_the_list_as_set() {
    // Arrange
    // The element path descends into the list, so the set state has to read
    // from the level that holds the list's own key.
    let text = "modes:\n  - \"enforce\"\n";

    // Act
    let body = yaml_hover_body(text, "enforce").expect("a hover is produced");

    // Assert
    assert!(
        body.contains("Set by the configuration."),
        "the cursor sits on the list's own value, body: {body}"
    );
}
