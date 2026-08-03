//! The handwritten root, end to end.
//!
//! One type and five impls: `FromFields`, an inherent `build` that both write
//! walks share, `ToFields`, `Validate`, and `ValidateNested`. Every helper the
//! read half uses is the one `#[derive(Spec)]` would have called from generated
//! code.
//!
//! `name` and `limits` are guarded against a repeat, through `first_occurrence`
//! and `parse_single_struct`. The derive guards every field it generates, so a
//! production parser wraps the rest the same way. The two here show the shape
//! for a leaf and for a block without repeating it eight more times.

use crate::children::{LimitsSpec, RouteSpec, TelemetrySpec};
use confval::format::{
    Fields, FieldsBuilder, FromFields, ToFields, Walk, first_occurrence, parse_bool_field,
    parse_float_field, parse_int_field, parse_path_field, parse_single_struct, parse_string_field,
    parse_string_list_field, parse_struct_field, parse_struct_list_field, report_missing_field,
    report_unknown_field,
};
use confval::prelude::*;
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
        FieldsBuilder::new(walk)
            .leaf("name", &self.name)
            .leaf("workers", &self.workers)
            .leaf("sample_rate", &self.sample_rate)
            .leaf("verbose", &self.verbose)
            .leaf_opt("pid_file", self.pid_file.as_ref())
            .string_list("events", &self.events)
            .string_list_opt("phases", self.phases.as_ref())
            .block("limits", &self.limits)
            .block_opt("telemetry", self.telemetry.as_ref())
            .block_list("route", &self.routes)
            .finish()
    }
}

impl ToFields for ServiceSpec {
    fn to_fields(&self) -> Fields {
        self.build(Walk::Populated)
    }

    fn to_source_fields(&self) -> Fields {
        self.build(Walk::Source)
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
