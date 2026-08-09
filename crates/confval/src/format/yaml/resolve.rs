//! The YAML 1.2 core schema resolution for plain scalars.
//!
//! YAML scalars are untyped text, and a schema decides what a plain one means.
//! The event stream hands the frontend the text and the style, so the frontend
//! owns this step. The core schema is the 1.2 default, and the patterns it
//! defines are quoted above each check, so an edge case is settled by the
//! pattern rather than by a rule of thumb.
//!
//! Anything the patterns do not match is a string. That is what retires the
//! Norway problem: `no` is not in the 1.2 boolean pattern, so `country: no` is
//! the string `no`.
//!
//! The module also decides what a style and a tag mean, because those are the
//! other two inputs to the same question. A quoted scalar is a string whatever
//! its text, and a tag either forces a reading, restates the node's own kind,
//! or is one the model has no place for.

use crate::format::field::{Scalar, ValueKind};
use saphyr_parser::{ScalarStyle, Tag};

/// The label every tag the frontend refuses carries.
pub(super) const TAGGED: &str = "tagged value";

/// What the core schema resolves a plain scalar's text to.
///
/// Two of these are outside the neutral model. An integer the pattern accepts
/// but `i64` cannot hold is [`OversizedInt`](Core::OversizedInt), and a float
/// whose text is a finite decimal but whose `f64` value overflows is
/// [`OversizedFloat`](Core::OversizedFloat). Both surface as type mismatches
/// rather than as distorted numbers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Core {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    OversizedInt,
    OversizedFloat,
    Str,
}

/// Resolves one plain scalar's text against the core schema.
pub(super) fn resolve(text: &str) -> Core {
    if let Some(core) = null(text)
        .or_else(|| boolean(text))
        .or_else(|| integer(text))
        .or_else(|| float(text))
    {
        return core;
    }
    Core::Str
}

/// Resolves text the way one core schema tag names, or `None` when the text is
/// not in that tag's pattern. `!!str` is not here, because it forces the string
/// reading rather than resolving anything.
pub(super) fn resolve_as(suffix: &str, text: &str) -> Option<Core> {
    match suffix {
        "null" => null(text),
        "bool" => boolean(text),
        "int" => integer(text),
        "float" => float(text),
        _ => None,
    }
}

/// `null | Null | NULL | ~` and the empty value.
fn null(text: &str) -> Option<Core> {
    matches!(text, "" | "~" | "null" | "Null" | "NULL").then_some(Core::Null)
}

/// `true | True | TRUE | false | False | FALSE`.
fn boolean(text: &str) -> Option<Core> {
    match text {
        "true" | "True" | "TRUE" => Some(Core::Bool(true)),
        "false" | "False" | "FALSE" => Some(Core::Bool(false)),
        _ => None,
    }
}

/// `[-+]? [0-9]+` in decimal, `0o [0-7]+` in octal, and `0x [0-9a-fA-F]+` in
/// hexadecimal.
///
/// The two base prefixes carry no sign and are lowercase, so `-0x10` and `0X1F`
/// are strings. A decimal beyond `i64` is [`Core::OversizedInt`], and so is a
/// based literal beyond it.
fn integer(text: &str) -> Option<Core> {
    if let Some(digits) = text.strip_prefix("0x") {
        return based(digits, 16, |character| character.is_ascii_hexdigit());
    }
    if let Some(digits) = text.strip_prefix("0o") {
        return based(digits, 8, |character| character.is_digit(8));
    }
    let digits = text.strip_prefix(['-', '+']).unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(match text.parse::<i64>() {
        Ok(int) => Core::Int(int),
        Err(_) => Core::OversizedInt,
    })
}

/// One based integer literal, once its prefix is stripped.
fn based(digits: &str, radix: u32, allowed: impl Fn(char) -> bool) -> Option<Core> {
    if digits.is_empty() || !digits.chars().all(allowed) {
        return None;
    }
    Some(match i64::from_str_radix(digits, radix) {
        Ok(int) => Core::Int(int),
        Err(_) => Core::OversizedInt,
    })
}

/// `[-+]? ( \. [0-9]+ | [0-9]+ ( \. [0-9]* )? ) ( [eE] [-+]? [0-9]+ )?`, plus
/// `[-+]? ( \.inf | \.Inf | \.INF )` and `\.nan | \.NaN | \.NAN`.
///
/// The infinity pattern carries an optional sign and the not-a-number pattern
/// does not, so `-.nan` is a string. Each has exactly three case variants, so
/// `.iNf` is a string too. A decimal whose `f64` value overflows, such as
/// `1e999`, is [`Core::OversizedFloat`].
fn float(text: &str) -> Option<Core> {
    let unsigned = text.strip_prefix(['-', '+']).unwrap_or(text);
    if matches!(unsigned, ".inf" | ".Inf" | ".INF") {
        let sign = if text.starts_with('-') { -1.0 } else { 1.0 };
        return Some(Core::Float(sign * f64::INFINITY));
    }
    if matches!(text, ".nan" | ".NaN" | ".NAN") {
        return Some(Core::Float(f64::NAN));
    }
    if !decimal(unsigned) {
        return None;
    }
    Some(match text.parse::<f64>() {
        Ok(float) if float.is_finite() => Core::Float(float),
        _ => Core::OversizedFloat,
    })
}

/// Whether an unsigned scalar matches the decimal float pattern: a mantissa of
/// either `. [0-9]+` or `[0-9]+ ( . [0-9]* )?`, then an optional exponent of
/// `[eE] [-+]? [0-9]+`.
fn decimal(text: &str) -> bool {
    let (mantissa, exponent) = match text.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, Some(exponent)),
        None => (text, None),
    };
    if let Some(exponent) = exponent {
        let digits = exponent.strip_prefix(['-', '+']).unwrap_or(exponent);
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return false;
        }
    }
    match mantissa.split_once('.') {
        // `.5` needs a fraction. `1.` and `1.5` do not.
        Some((whole, fraction)) => {
            let digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
            if whole.is_empty() {
                return !fraction.is_empty() && digits(fraction);
            }
            digits(whole) && digits(fraction)
        }
        // A bare integer mantissa is a float only with an exponent, because the
        // integer pattern is tried first and matches it otherwise.
        None => !mantissa.is_empty() && mantissa.bytes().all(|byte| byte.is_ascii_digit()),
    }
}

/// Whether a collection's tag leaves the node reading as itself.
///
/// An absent tag and the non-specific `!` both do. So does the core schema tag
/// for the node's own kind, which restates what the shape already says. Any
/// other tag is refused, because the model has no place to put it.
pub(super) fn reads_through(tag: Option<&Tag>, kind: &str) -> bool {
    match tag {
        None => true,
        Some(tag) if non_specific(tag) => true,
        Some(tag) => tag.is_yaml_core_schema() && tag.suffix == kind,
    }
}

/// Whether a tag is YAML's non-specific `!`, which the schema resolves by node
/// kind: a string on a scalar, and the node itself on a collection.
fn non_specific(tag: &Tag) -> bool {
    tag.handle.is_empty() && tag.suffix == "!"
}

/// One scalar's neutral value kind, from its text, its style, and its tag.
pub(super) fn scalar_kind(text: &str, style: ScalarStyle, tag: Option<&Tag>) -> ValueKind {
    let Some(tag) = tag else {
        return match style {
            ScalarStyle::Plain => of_core(resolve(text), text),
            // A quoted, literal, or folded scalar is a string whatever its
            // text, so `port: "8080"` is the string `8080`.
            _ => string(text),
        };
    };
    if non_specific(tag) {
        return string(text);
    }
    if !tag.is_yaml_core_schema() {
        return ValueKind::Other(TAGGED);
    }
    match tag.suffix.as_str() {
        "str" => string(text),
        suffix @ ("null" | "bool" | "int" | "float") => match resolve_as(suffix, text) {
            Some(core) => of_core(core, text),
            None => ValueKind::Other(TAGGED),
        },
        // A core tag naming a collection sits on the wrong node kind here, and
        // an unknown `!!name` has no reading at all.
        _ => ValueKind::Other(TAGGED),
    }
}

/// The neutral value kind for one resolved scalar.
fn of_core(core: Core, text: &str) -> ValueKind {
    match core {
        Core::Null => ValueKind::Other("null"),
        Core::Bool(value) => ValueKind::Scalar(Scalar::Bool(value)),
        Core::Int(value) => ValueKind::Scalar(Scalar::Int(value)),
        Core::Float(value) => ValueKind::Scalar(Scalar::Float(value)),
        Core::OversizedInt => ValueKind::Other("oversized integer"),
        Core::OversizedFloat => ValueKind::Other("oversized number"),
        Core::Str => string(text),
    }
}

fn string(text: &str) -> ValueKind {
    ValueKind::Scalar(Scalar::String(text.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_null_form_resolves_to_null() {
        // Arrange
        let forms = ["", "~", "null", "Null", "NULL"];

        // Act
        let resolved: Vec<Core> = forms.iter().map(|text| resolve(text)).collect();

        // Assert
        assert!(
            resolved.iter().all(|core| *core == Core::Null),
            "{resolved:?}"
        );
    }

    #[test]
    fn every_boolean_form_resolves_to_its_value() {
        // Arrange
        let forms = [
            ("true", true),
            ("True", true),
            ("TRUE", true),
            ("false", false),
            ("False", false),
            ("FALSE", false),
        ];

        // Act
        let resolved: Vec<Core> = forms.iter().map(|(text, _)| resolve(text)).collect();

        // Assert
        let expected: Vec<Core> = forms.iter().map(|(_, value)| Core::Bool(*value)).collect();
        assert_eq!(resolved, expected);
    }

    #[test]
    fn the_yaml_1_1_booleans_are_strings() {
        // Arrange
        // These four are the Norway problem. The 1.2 core schema drops them, so
        // `country: no` is the string `no`.
        let forms = [
            "yes", "Yes", "YES", "no", "No", "NO", "on", "On", "ON", "off", "Off", "OFF", "y", "n",
        ];

        // Act
        let resolved: Vec<Core> = forms.iter().map(|text| resolve(text)).collect();

        // Assert
        assert!(
            resolved.iter().all(|core| *core == Core::Str),
            "{resolved:?}"
        );
    }

    #[test]
    fn decimal_integers_resolve_with_either_sign() {
        // Arrange
        let forms = [("0", 0), ("7", 7), ("+1", 1), ("-1", -1), ("007", 7)];

        // Act
        let resolved: Vec<Core> = forms.iter().map(|(text, _)| resolve(text)).collect();

        // Assert
        let expected: Vec<Core> = forms.iter().map(|(_, value)| Core::Int(*value)).collect();
        assert_eq!(resolved, expected);
    }

    #[test]
    fn lowercase_based_integers_resolve() {
        // Arrange
        let forms = [("0x1f", 31), ("0xFF", 255), ("0o17", 15)];

        // Act
        let resolved: Vec<Core> = forms.iter().map(|(text, _)| resolve(text)).collect();

        // Assert
        let expected: Vec<Core> = forms.iter().map(|(_, value)| Core::Int(*value)).collect();
        assert_eq!(resolved, expected);
    }

    #[test]
    fn a_signed_or_uppercase_base_prefix_is_a_string() {
        // Arrange
        // The core patterns carry no sign on a based literal and write the
        // prefix lowercase, so each of these near-misses is text.
        let forms = ["-0x10", "+0x10", "0X1F", "0O17", "0x", "0o", "0xzz"];

        // Act
        let resolved: Vec<Core> = forms.iter().map(|text| resolve(text)).collect();

        // Assert
        assert!(
            resolved.iter().all(|core| *core == Core::Str),
            "{resolved:?}"
        );
    }

    #[test]
    fn an_underscored_number_is_a_string() {
        // Arrange
        // The digit separator is a 1.1 convention the core schema dropped.
        let forms = ["1_000", "1_0.5", "0x_ff"];

        // Act
        let resolved: Vec<Core> = forms.iter().map(|text| resolve(text)).collect();

        // Assert
        assert!(
            resolved.iter().all(|core| *core == Core::Str),
            "{resolved:?}"
        );
    }

    #[test]
    fn an_integer_beyond_i64_is_oversized() {
        // Arrange
        // i128 holds this, i64 does not.
        let forms = [
            "9223372036854775808",
            "-9223372036854775809",
            "0xFFFFFFFFFFFFFFFF",
        ];

        // Act
        let resolved: Vec<Core> = forms.iter().map(|text| resolve(text)).collect();

        // Assert
        assert!(
            resolved.iter().all(|core| *core == Core::OversizedInt),
            "{resolved:?}"
        );
    }

    #[test]
    fn decimal_floats_resolve_with_a_point_or_an_exponent() {
        // Arrange
        let forms = [
            ("1.5", 1.5),
            ("+1.5", 1.5),
            ("-1.5", -1.5),
            (".5", 0.5),
            ("1.", 1.0),
            ("1e3", 1000.0),
            ("1E3", 1000.0),
            ("-1e-3", -0.001),
            ("1.5e2", 150.0),
        ];

        // Act
        let resolved: Vec<Core> = forms.iter().map(|(text, _)| resolve(text)).collect();

        // Assert
        let expected: Vec<Core> = forms.iter().map(|(_, value)| Core::Float(*value)).collect();
        assert_eq!(resolved, expected);
    }

    #[test]
    fn a_bare_integer_resolves_as_an_integer_rather_than_a_float() {
        // Arrange
        // Both patterns accept `1`. The schema tries the integer first.
        let text = "1";

        // Act
        let resolved = resolve(text);

        // Assert
        assert_eq!(resolved, Core::Int(1));
    }

    #[test]
    fn every_infinity_form_resolves_with_its_sign() {
        // Arrange
        let forms = [
            (".inf", f64::INFINITY),
            (".Inf", f64::INFINITY),
            (".INF", f64::INFINITY),
            ("+.inf", f64::INFINITY),
            ("-.inf", f64::NEG_INFINITY),
            ("-.INF", f64::NEG_INFINITY),
        ];

        // Act
        let resolved: Vec<Core> = forms.iter().map(|(text, _)| resolve(text)).collect();

        // Assert
        let expected: Vec<Core> = forms.iter().map(|(_, value)| Core::Float(*value)).collect();
        assert_eq!(resolved, expected);
    }

    #[test]
    fn every_not_a_number_form_resolves_unsigned() {
        // Arrange
        let forms = [".nan", ".NaN", ".NAN"];

        // Act
        let resolved: Vec<Core> = forms.iter().map(|text| resolve(text)).collect();

        // Assert
        for (text, core) in forms.iter().zip(&resolved) {
            let Core::Float(float) = core else {
                panic!("{text} should resolve to a float, got {core:?}");
            };
            assert!(float.is_nan(), "{text} should be NaN");
        }
    }

    #[test]
    fn a_signed_or_miscased_not_a_number_is_a_string() {
        // Arrange
        // The core pattern carries no sign and lists three case variants, so
        // each of these is text.
        let forms = [
            "-.nan", "+.nan", ".Nan", ".nAn", "nan", "NaN", ".iNf", "inf",
        ];

        // Act
        let resolved: Vec<Core> = forms.iter().map(|text| resolve(text)).collect();

        // Assert
        assert!(
            resolved.iter().all(|core| *core == Core::Str),
            "{resolved:?}"
        );
    }

    #[test]
    fn a_float_beyond_f64_is_oversized() {
        // Arrange
        // The pattern accepts the text and `f64` has no finite value for it, so
        // the model refuses it rather than holding infinity.
        let forms = ["1e999", "-1e999", "1.5e400"];

        // Act
        let resolved: Vec<Core> = forms.iter().map(|text| resolve(text)).collect();

        // Assert
        assert!(
            resolved.iter().all(|core| *core == Core::OversizedFloat),
            "{resolved:?}"
        );
    }

    #[test]
    fn ordinary_text_is_a_string() {
        // Arrange
        let forms = ["hostname", "127.0.0.1", "a b", "", " "]
            .into_iter()
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>();

        // Act
        let resolved: Vec<Core> = forms.iter().map(|text| resolve(text)).collect();

        // Assert
        assert!(
            resolved.iter().all(|core| *core == Core::Str),
            "{resolved:?}"
        );
    }

    #[test]
    fn a_core_tag_resolves_only_the_text_its_pattern_accepts() {
        // Arrange
        // `resolve_as` answers `None` for text the tag cannot read, which the
        // frontend turns into a `tagged value` mismatch.
        let cases = [
            ("int", "8080", Some(Core::Int(8080))),
            ("int", "foo", None),
            ("float", "1.5", Some(Core::Float(1.5))),
            ("float", "foo", None),
            ("bool", "true", Some(Core::Bool(true))),
            ("bool", "yes", None),
            ("null", "~", Some(Core::Null)),
            ("null", "0", None),
            ("map", "anything", None),
        ];

        // Act
        let resolved: Vec<Option<Core>> = cases
            .iter()
            .map(|(suffix, text, _)| resolve_as(suffix, text))
            .collect();

        // Assert
        let expected: Vec<Option<Core>> = cases.iter().map(|(_, _, core)| *core).collect();
        assert_eq!(resolved, expected);
    }

    #[test]
    fn a_core_int_tag_keeps_the_oversized_reading() {
        // Arrange
        // The tag can read the text, but the model cannot hold the value, so
        // the label names the magnitude rather than the tag.
        let text = "9223372036854775808";

        // Act
        let resolved = resolve_as("int", text);

        // Assert
        assert_eq!(resolved, Some(Core::OversizedInt));
    }
}
