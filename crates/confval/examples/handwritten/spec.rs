//! The handwritten root, end to end.
//!
//! One type and six impls: `FromFields`, an inherent `build` that both write
//! walks share, `ToFields`, `ToSchema`, `Validate`, and `ValidateNested`. Every
//! helper the read half uses is the one `#[derive(Spec)]` would have called from
//! generated code.
//!
//! `name` and `limits` are guarded against a repeat, through `first_occurrence`
//! and `parse_single_struct`. The derive guards every field it generates, so a
//! production parser wraps the rest the same way. The two here show the shape
//! for a leaf and for a block without repeating it eight more times.
//!
//! The `headers` map shows a third shape. The derive can express a string map,
//! but the handwritten write path, `FieldsBuilder`, has no method for one, so
//! this impl reads it with `parse_string_map_field` and builds the field to
//! push by hand.

use crate::children::{LimitsSpec, RouteSpec, TelemetrySpec};
use confval::format::{
    Field, Fields, FieldsBuilder, FromFields, Scalar, ToFields, Value, ValueKind, Walk,
    first_occurrence, parse_bool_field, parse_float_field, parse_int_field, parse_path_field,
    parse_single_struct, parse_string_field, parse_string_list_field, parse_string_map_field,
    parse_struct_field, parse_struct_list_field, report_missing_field, report_unknown_field,
};
use confval::prelude::*;
use confval::schema::{Constraint, ScalarType, Schema, SchemaField, SchemaType};
use std::collections::BTreeMap;
use std::path::PathBuf;

range_constraint!(WORKERS, i64, min: 1, max: 512);

confval::keyword_enum!(pub LogEvent, {
    Started  => "started",
    Reloaded => "reloaded",
    Stopped  => "stopped",
});

confval::keyword_enum!(pub Phase, {
    Request  => "request",
    Response => "response",
});

/// The root of the tree. Its Rust field names and its config keys are chosen
/// separately, which is why `routes` reads a `route` block.
#[derive(Debug)]
pub struct ServiceSpec {
    pub name: Located<String>,
    pub workers: Located<i64>,
    pub sample_rate: Located<f64>,
    pub verbose: Located<bool>,
    pub pid_file: Option<Located<PathBuf>>,
    pub events: Vec<Located<String>>,
    pub phases: Option<Located<Vec<Located<String>>>>,
    pub headers: BTreeMap<String, Located<String>>,
    pub limits: Located<LimitsSpec>,
    pub telemetry: Option<Located<TelemetrySpec>>,
    pub routes: Vec<Located<RouteSpec>>,
}

impl FromFields for ServiceSpec {
    fn from_fields(fields: &Fields, report: &mut Report) -> Option<Self> {
        let mut name = None;
        let mut name_seen = None;
        let mut workers = None;
        let mut sample_rate = None;
        let mut verbose = None;
        let mut pid_file = None;
        let mut events = None;
        let mut phases = None;
        let mut headers = None;
        let mut headers_seen = None;
        let mut limits = None;
        let mut limits_seen = None;
        let mut telemetry = None;
        let mut routes = Vec::new();

        for field in fields.iter() {
            match field.name.as_str() {
                // A repeated leaf keeps the first occurrence and reports the
                // second against both spans.
                "name" => {
                    if first_occurrence(&mut name_seen, "name", field, report) {
                        name = parse_string_field(field, report);
                    }
                }
                "workers" => workers = parse_int_field(field, report),
                "sample_rate" => sample_rate = parse_float_field(field, report),
                "verbose" => verbose = parse_bool_field(field, report),
                "pid_file" => pid_file = parse_path_field(field, report),
                "events" => events = parse_string_list_field(field, report),
                "phases" => phases = parse_string_list_field(field, report),
                // The map is single-occurrence, like a leaf. The read uses the
                // public helper the derive would have called.
                "headers" => {
                    if first_occurrence(&mut headers_seen, "headers", field, report) {
                        headers = parse_string_map_field(field, report);
                    }
                }
                // A single-occurrence block reports a repeat the same way.
                "limits" => {
                    parse_single_struct(&mut limits, &mut limits_seen, "limits", field, report);
                }
                "telemetry" => telemetry = parse_struct_field(field, report),
                "route" => parse_struct_list_field(&mut routes, field, report),
                _ => report_unknown_field(field, report),
            }
        }

        // A required field that never parsed is reported against the enclosing
        // level, unless the field was there and already reported for its own
        // reason.
        if name.is_none() && !fields.has("name") {
            report_missing_field("name", fields.enclosing(), report);
        }
        if limits.is_none() && !fields.has("limits") {
            report_missing_field("limits", fields.enclosing(), report);
        }

        Some(ServiceSpec {
            name: name?,
            // An absent default fills detached, which is what marks it as a
            // default for the source view.
            workers: workers.unwrap_or(Located::detached(4)),
            sample_rate: sample_rate.unwrap_or(Located::detached(1.0)),
            verbose: verbose.unwrap_or(Located::detached(false)),
            pid_file,
            events: events.map(|list| list.value).unwrap_or_default(),
            phases,
            headers: headers.map(|map| map.value).unwrap_or_default(),
            limits: limits?,
            telemetry,
            routes,
        })
    }
}

impl ServiceSpec {
    /// The field list both write walks read. `Walk` decides what each method
    /// does with each span it is given, so the list is written once.
    fn build(&self, walk: Walk) -> Fields {
        let mut builder = FieldsBuilder::new(walk)
            .leaf("name", &self.name)
            .leaf("workers", &self.workers)
            .leaf("sample_rate", &self.sample_rate)
            .leaf("verbose", &self.verbose)
            .leaf_opt("pid_file", self.pid_file.as_ref())
            .string_list("events", &self.events)
            .string_list_opt("phases", self.phases.as_ref());
        // The builder shapes no map, so the field is built here and pushed. The
        // walk decides whether a detached entry, one no source wrote, is kept.
        if let Some(field) = string_map_field(walk, "headers", &self.headers) {
            builder = builder.push(field);
        }
        builder
            .block("limits", &self.limits)
            .block_opt("telemetry", self.telemetry.as_ref())
            .block_list("route", &self.routes)
            .finish()
    }
}

/// Builds the `headers` map field for one walk.
///
/// The populated walk emits every entry with a detached value, the way
/// `to_fields` does. The source walk keeps only the entries a source wrote,
/// each with its span, and drops the field when none remain, the way
/// `to_source_fields` does. A `BTreeMap` emits in key order, so the field is
/// deterministic.
fn string_map_field(
    walk: Walk,
    name: &str,
    map: &BTreeMap<String, Located<String>>,
) -> Option<Field> {
    let source = matches!(walk, Walk::Source);
    let mut entries = Vec::new();
    for (key, value) in map {
        if source && value.span.is_detached() {
            continue;
        }
        let scalar = ValueKind::Scalar(Scalar::String(value.value.clone()));
        let inner = if source {
            Value::spanned(value.span, scalar)
        } else {
            Value::detached(scalar)
        };
        entries.push(Field::detached_value(key, inner));
    }
    if source && entries.is_empty() {
        return None;
    }
    Some(Field::detached_value(
        name,
        Value::detached(ValueKind::Map(Fields::detached(entries))),
    ))
}

impl ToFields for ServiceSpec {
    fn to_fields(&self) -> Fields {
        self.build(Walk::Populated)
    }

    fn to_source_fields(&self) -> Fields {
        self.build(Walk::Source)
    }
}

/// The type-level schema, written by hand the way `#[derive(Spec)]` would emit
/// it. `SchemaField::new` folds each field's structural requiredness and its
/// default into the `required` a consumer reads, so a defaulted field passes
/// `true, true` and is not required. `route` is the `routes` field's key. Every
/// node is built through the `Schema::new` and `SchemaField::new` constructors,
/// because the node structs are `#[non_exhaustive]`.
impl ToSchema for ServiceSpec {
    fn schema() -> Schema {
        let block = |schema: Schema, repeated: bool| SchemaType::Block {
            schema: Box::new(schema),
            repeated,
        };
        let leaf = |leaf| SchemaType::Scalar {
            leaf,
            constraint: None,
        };
        let sf = |name: &str, structurally_required: bool, has_default: bool, ty: SchemaType| {
            SchemaField::new(
                name.to_string(),
                None,
                structurally_required,
                has_default,
                ty,
            )
        };
        let workers = SchemaType::Scalar {
            leaf: ScalarType::Int,
            constraint: Some(Constraint::Range {
                min: WORKERS.min.to_string(),
                max: WORKERS.max.to_string(),
                units: WORKERS.units,
                help: WORKERS.help,
            }),
        };
        Schema::new(
            None,
            vec![
                sf("name", true, false, leaf(ScalarType::String)),
                sf("workers", true, true, workers),
                sf("sample_rate", true, true, leaf(ScalarType::Float)),
                sf("verbose", true, true, leaf(ScalarType::Bool)),
                sf("pid_file", false, false, leaf(ScalarType::Path)),
                sf("events", true, true, SchemaType::StringList),
                sf("phases", false, false, SchemaType::StringList),
                sf("headers", true, true, SchemaType::StringMap),
                sf("limits", true, false, block(LimitsSpec::schema(), false)),
                sf(
                    "telemetry",
                    false,
                    false,
                    block(TelemetrySpec::schema(), false),
                ),
                sf("route", false, false, block(RouteSpec::schema(), true)),
            ],
        )
    }
}

impl Validate for ServiceSpec {
    fn validate(&self, report: &mut Report) {
        WORKERS.check_located(&self.workers, "workers", report);
        // A keyword list is checked per element, so a typo is reported under
        // the entry the operator typed.
        LogEvent::keyword_set().check_each(&self.events, "event", report);
        if let Some(phases) = &self.phases {
            Phase::keyword_set().check_each(&phases.value, "phase", report);
        }
        // A per-entry rule over the map. Each value keeps its span, so an empty
        // header value is reported at the entry the operator wrote.
        for (key, value) in &self.headers {
            if value.value.is_empty() {
                report
                    .error(format!("header \"{key}\" must not be empty"))
                    .at(value.span)
                    .emit();
            }
        }
    }
}

/// The traversal the derive would have generated. Written by hand it descends
/// the same way. The `Self: ValidateNested` bound on `validate_all` makes
/// leaving it out a compile error rather than a silently skipped subtree.
impl ValidateNested for ServiceSpec {
    fn validate_nested(&self, report: &mut Report) {
        self.limits.value.validate_all(report);
        if let Some(telemetry) = &self.telemetry {
            telemetry.value.validate_all(report);
        }
        for route in &self.routes {
            route.value.validate_all(report);
        }
    }
}
