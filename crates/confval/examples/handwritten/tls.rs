//! A tagged block, written by hand to show a spec without the derive.
//!
//! `mode` decides which fields the rest of the block has. This type implements
//! the five traits `#[derive(Spec)]` would generate, plus `Default`, which a
//! required nested slot needs.
//!
//! It is a field of `RouteSpec`, which is derived. The generated parser calls
//! `TlsSpec::from_fields`, the generated write walks call its `to_fields` and
//! `to_source_fields`, and the generated `schema()` calls its `ToSchema`. None
//! of them distinguishes it from a derived type, so a handwritten child nested
//! under a derived parent implements `ToSchema` by hand or the parent does not
//! compile.

use confval::format::{
    Fields, FieldsBuilder, FromFields, ToFields, Walk, parse_path_field, parse_string_field,
    parse_string_list_field, report_missing_field, report_unknown_field,
};
use confval::prelude::*;
use confval::schema::{Constraint, ScalarType, Schema, SchemaField, SchemaType};
use std::path::PathBuf;

confval::keyword_enum!(pub TlsChallenge, {
    Http01 => "http-01",
    Dns01  => "dns-01",
});

/// The tag's own closed set. `from_fields` matches these strings to decide the
/// variant. The schema records them so an editor can complete the discriminant,
/// matching what the recording attributes deliver for a derived spec. The two
/// places are unguarded, so a value added here must be added to the `from_fields`
/// match by hand.
const TLS_MODES: [&str; 2] = ["manual", "acme"];

/// A tagged block: `mode = "manual"` takes a certificate pair, and
/// `mode = "acme"` takes a domain list and a challenge type.
#[derive(Debug)]
pub enum TlsSpec {
    Manual {
        cert: Located<PathBuf>,
        key: Located<PathBuf>,
    },
    Acme {
        domains: Vec<Located<String>>,
        challenge: Located<String>,
    },
}

/// A handwritten type in a required nested slot also needs `Default`. The
/// generated parser fills an absent block with it before reporting the block
/// missing, so the value is never observed on a document that parses. A tagged
/// enum has no natural default, so this one uses `Manual` with two empty paths.
impl Default for TlsSpec {
    fn default() -> Self {
        TlsSpec::Manual {
            cert: Located::detached(PathBuf::new()),
            key: Located::detached(PathBuf::new()),
        }
    }
}

impl FromFields for TlsSpec {
    fn from_fields(fields: &Fields, report: &mut Report) -> Option<Self> {
        // The tag decides which fields are legal, so it is read before the walk
        // rather than during it.
        let Some(mode_field) = fields.get("mode") else {
            report_missing_field("mode", fields.enclosing(), report);
            return None;
        };
        let mode = parse_string_field(mode_field, report)?;

        match mode.value.as_str() {
            "manual" => {
                let mut cert = None;
                let mut key = None;
                for field in fields.iter() {
                    match field.name.as_str() {
                        "mode" => {}
                        "cert" => cert = parse_path_field(field, report),
                        "key" => key = parse_path_field(field, report),
                        _ => report_unknown_field(field, report),
                    }
                }
                if cert.is_none() && !fields.has("cert") {
                    report_missing_field("cert", fields.enclosing(), report);
                }
                if key.is_none() && !fields.has("key") {
                    report_missing_field("key", fields.enclosing(), report);
                }
                Some(TlsSpec::Manual {
                    cert: cert?,
                    key: key?,
                })
            }
            "acme" => {
                let mut domains = None;
                let mut challenge = None;
                for field in fields.iter() {
                    match field.name.as_str() {
                        "mode" => {}
                        "domains" => domains = parse_string_list_field(field, report),
                        "challenge" => challenge = parse_string_field(field, report),
                        _ => report_unknown_field(field, report),
                    }
                }
                if challenge.is_none() && !fields.has("challenge") {
                    report_missing_field("challenge", fields.enclosing(), report);
                }
                Some(TlsSpec::Acme {
                    domains: domains.map(|list| list.value).unwrap_or_default(),
                    challenge: challenge?,
                })
            }
            other => {
                report
                    .error(format!("unknown mode: {other}"))
                    .at(mode.span)
                    .help("expected one of: manual, acme")
                    .emit();
                None
            }
        }
    }
}

impl TlsSpec {
    fn build(&self, walk: Walk) -> Fields {
        let fields = FieldsBuilder::new(walk);
        // The tag has no `Located` behind it, because the variant carries it.
        // Both walks emit it: a source view without the tag would not reparse.
        match self {
            TlsSpec::Manual { cert, key } => fields
                .literal_string("mode", "manual")
                .leaf("cert", cert)
                .leaf("key", key),
            TlsSpec::Acme { domains, challenge } => fields
                .literal_string("mode", "acme")
                .string_list("domains", domains)
                .leaf("challenge", challenge),
        }
        .finish()
    }
}

impl ToFields for TlsSpec {
    fn to_fields(&self) -> Fields {
        self.build(Walk::Populated)
    }

    fn to_source_fields(&self) -> Fields {
        self.build(Walk::Source)
    }
}

impl Validate for TlsSpec {
    fn validate(&self, report: &mut Report) {
        match self {
            TlsSpec::Manual { .. } => {}
            TlsSpec::Acme { domains, challenge } => {
                TlsChallenge::keyword_set().check_located(challenge, "challenge", report);
                if domains.is_empty() {
                    report
                        .error("acme mode needs at least one domain")
                        .at(challenge.span)
                        .help("Add a domains list, e.g. domains = [\"example.com\"].")
                        .emit();
                }
            }
        }
    }
}

/// A type with no nested children still implements the traversal, because
/// `validate_all` requires the trait on every spec type, including the leaves.
impl ValidateNested for TlsSpec {
    fn validate_nested(&self, _report: &mut Report) {}
}

/// The type-level schema, written by hand the way `#[derive(Spec)]` would emit
/// it. A tag decides which fields a variant has, so any one instance shows only
/// one variant's fields. The schema lists `mode` and every field a variant can
/// carry, each built through the `Schema::new` and `SchemaField::new`
/// constructors, because the node structs are `#[non_exhaustive]`.
impl ToSchema for TlsSpec {
    fn schema() -> Schema {
        Schema::new(
            None,
            vec![
                SchemaField::new(
                    "mode".to_string(),
                    None,
                    SchemaType::Scalar {
                        leaf: ScalarType::String,
                        constraint: Some(Constraint::Keywords(&TLS_MODES)),
                    },
                )
                .required(),
                SchemaField::new(
                    "cert".to_string(),
                    None,
                    SchemaType::Scalar {
                        leaf: ScalarType::Path,
                        constraint: None,
                    },
                ),
                SchemaField::new(
                    "key".to_string(),
                    None,
                    SchemaType::Scalar {
                        leaf: ScalarType::Path,
                        constraint: None,
                    },
                ),
                SchemaField::new(
                    "domains".to_string(),
                    None,
                    SchemaType::StringList { constraint: None },
                ),
                SchemaField::new(
                    "challenge".to_string(),
                    None,
                    SchemaType::Scalar {
                        leaf: ScalarType::String,
                        constraint: Some(Constraint::Keywords(&TlsChallenge::KEYWORDS)),
                    },
                ),
            ],
        )
    }
}
