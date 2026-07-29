//! Assemble one configuration from several layered sources.
//!
//! A file supplies the base, environment variables override it, and command
//! line flags override those. Every provider yields the neutral
//! [`Fields`], so the file frontends, the environment provider, and the
//! command line provider are ordinary functions that differ only in the
//! substrate they read.
//!
//! [`Assembly`] collects the layers and folds them. Precedence is the call
//! order. [`merge`](Assembly::merge) lets a later layer override an earlier
//! one, and [`join`](Assembly::join) lets an earlier layer stand while a later
//! one fills what is missing. [`FromFields`] runs once, on the merged result.
//!
//! ```no_run
//! use confval::layering::{Assembly, cli_fields, env_fields};
//! # use confval::source::SourceMap;
//! # use confval::diagnostic::Report;
//! # use confval::format::Fields;
//! # fn demo<T: confval::format::FromFields>(sources: &mut SourceMap, base: Option<Fields>, args: Vec<String>, report: &mut Report) -> Option<T> {
//! // `base` came from a file frontend, `parse_toml_fields` or
//! // `parse_hcl_fields`, behind its format feature.
//! let spec = Assembly::new()
//!     .merge(base)
//!     .merge(env_fields(sources, "APP_", report))
//!     .merge(cli_fields(sources, args, report))
//!     .assemble::<T>(report);
//! # spec
//! # }
//! ```

mod cli;
mod env;
mod merge;
mod nesting;

pub use cli::cli_fields;
pub use env::env_fields;

use crate::diagnostic::Report;
use crate::format::{Fields, FromFields};
use merge::Verb;

/// A builder that layers configuration sources and folds them into one spec.
///
/// The builder borrows nothing. Each provider is a free function the caller
/// invokes, so a provider's `&mut Report` borrow ends before the next runs and
/// the chain never holds two mutable borrows of the report at once.
/// [`assemble`](Assembly::assemble) takes the report once, at the end.
#[derive(Default)]
pub struct Assembly {
    layers: Vec<Layer>,
}

struct Layer {
    verb: Verb,
    fields: Option<Fields>,
}

impl Assembly {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a layer whose values override earlier layers on overlap.
    pub fn merge(mut self, fields: Option<Fields>) -> Self {
        self.layers.push(Layer {
            verb: Verb::Merge,
            fields,
        });
        self
    }

    /// Adds a layer that fills only what earlier layers left missing.
    pub fn join(mut self, fields: Option<Fields>) -> Self {
        self.layers.push(Layer {
            verb: Verb::Join,
            fields,
        });
        self
    }

    /// Folds the layers and runs [`FromFields`] once on the result.
    ///
    /// Every provider has already run by the time this is called, so all
    /// syntax errors are in the report. When any provider produced no tree, a
    /// failed source, this returns `None` before `FromFields` runs, so the
    /// operator sees the syntax errors without a cascade of missing-field
    /// errors from the source that failed. Otherwise the layers fold left to
    /// right, the first as the base, and `FromFields` runs on the merged
    /// `Fields`.
    ///
    /// A `None` always arrives with at least one error in the report. When a
    /// layer is `None` and nothing was reported, or the assembly holds no
    /// layers, an internal error is reported so the caller is never left with
    /// a silent failure.
    pub fn assemble<T: FromFields>(self, report: &mut Report) -> Option<T> {
        if self.layers.is_empty() {
            report
                .error("internal error: assemble called with no layers")
                .emit();
            return None;
        }
        if self.layers.iter().any(|layer| layer.fields.is_none()) {
            if !report.has_errors() {
                report
                    .error("internal error: a layer produced no fields and reported no error")
                    .emit();
            }
            return None;
        }
        let mut layers = self.layers.into_iter();
        let mut merged = layers.next()?.fields?;
        for layer in layers {
            if let Some(fields) = layer.fields {
                merged = merge::combine(merged, fields, layer.verb, report);
            }
        }
        T::from_fields(&merged, report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::parse::{parse_int_field, parse_string_field};
    use crate::source::SourceMap;

    struct Server {
        host: String,
        port: u16,
    }

    impl FromFields for Server {
        fn from_fields(fields: &Fields, report: &mut Report) -> Option<Self> {
            let host = fields
                .get("host")
                .and_then(|field| parse_string_field(field, report));
            let port = fields
                .get("port")
                .and_then(|field| parse_int_field(field, report));
            match (host, port) {
                (Some(host), Some(port)) => Some(Server {
                    host: host.value,
                    port: port.value as u16,
                }),
                _ => {
                    report.error("missing host or port").emit();
                    None
                }
            }
        }
    }

    #[test]
    fn a_later_layer_overrides_an_earlier_one() {
        // Arrange
        let mut sources = SourceMap::new();
        let mut report = Report::new();
        let base = cli_fields(
            &mut sources,
            vec!["--host=filehost".to_string(), "--port=1".to_string()],
            &mut report,
        );
        let over = cli_fields(&mut sources, vec!["--port=2".to_string()], &mut report);

        // Act
        let server: Option<Server> = Assembly::new()
            .merge(base)
            .merge(over)
            .assemble(&mut report);

        // Assert
        let server = server.unwrap();
        assert_eq!(server.host, "filehost");
        assert_eq!(server.port, 2);
        assert!(!report.has_issues());
    }

    #[test]
    fn a_required_field_can_come_from_a_higher_layer() {
        // Arrange
        let mut sources = SourceMap::new();
        let mut report = Report::new();
        let base = cli_fields(&mut sources, vec!["--host=only".to_string()], &mut report);
        let over = cli_fields(&mut sources, vec!["--port=8080".to_string()], &mut report);

        // Act
        let server: Option<Server> = Assembly::new()
            .merge(base)
            .merge(over)
            .assemble(&mut report);

        // Assert
        assert_eq!(server.unwrap().port, 8080);
    }

    #[test]
    fn a_failed_source_stops_before_from_fields() {
        // Arrange
        let mut sources = SourceMap::new();
        let mut report = Report::new();
        let good = cli_fields(
            &mut sources,
            vec!["--host=h".to_string(), "--port=1".to_string()],
            &mut report,
        );

        // Act: a provider that produced no tree is a `None` layer.
        let server: Option<Server> = Assembly::new()
            .merge(good)
            .merge(None)
            .assemble(&mut report);

        // Assert
        // The provider contract is report-then-None. This layer reported
        // nothing, so assemble supplies the fallback error rather than
        // returning `None` with a clean report.
        assert!(server.is_none());
        assert!(report.has_errors());
        assert!(report.issues()[0].message.contains("reported no error"));
    }

    #[test]
    fn a_failed_source_with_a_reported_error_adds_nothing() {
        // Arrange
        let mut sources = SourceMap::new();
        let mut report = Report::new();
        let good = cli_fields(&mut sources, vec!["--host=h".to_string()], &mut report);
        report
            .error("syntax error: the provider already reported")
            .emit();

        // Act
        let server: Option<Server> = Assembly::new()
            .merge(good)
            .merge(None)
            .assemble(&mut report);

        // Assert
        assert!(server.is_none());
        assert_eq!(report.issues().len(), 1);
    }

    #[test]
    fn an_empty_assembly_reports_an_internal_error() {
        // Arrange
        let mut report = Report::new();

        // Act
        let server: Option<Server> = Assembly::new().assemble(&mut report);

        // Assert
        assert!(server.is_none());
        assert!(report.has_errors());
        assert!(report.issues()[0].message.contains("no layers"));
    }

    #[test]
    fn join_fills_a_missing_field_without_overriding() {
        // Arrange
        let mut sources = SourceMap::new();
        let mut report = Report::new();
        let base = cli_fields(
            &mut sources,
            vec!["--host=primary".to_string(), "--port=1".to_string()],
            &mut report,
        );
        let defaults = cli_fields(
            &mut sources,
            vec!["--host=fallback".to_string()],
            &mut report,
        );

        // Act
        let server: Option<Server> = Assembly::new()
            .merge(base)
            .join(defaults)
            .assemble(&mut report);

        // Assert
        assert_eq!(server.unwrap().host, "primary");
    }
}
