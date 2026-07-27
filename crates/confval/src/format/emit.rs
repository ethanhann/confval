//! The error both format emitters return.
//!
//! Emit serializes the neutral [`Fields`](crate::format::Fields) model back to a
//! format's text. Not every `Fields` is representable in every format, so emit
//! is fallible. A populated spec never hits either case, because populate builds
//! only identifier names and never a `ValueKind::Other`. The cases arise when
//! you emit a `Fields` a frontend parsed, which can carry a name or a value the
//! target format cannot spell.

use std::fmt::{self, Display, Formatter};

/// Why a `Fields` could not be emitted to a format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmitError {
    /// A name has no representation in the target format, such as a
    /// non-identifier attribute or block name in HCL. TOML quotes any key, so
    /// this arises only for HCL.
    UnrepresentableName(String),
    /// A `ValueKind::Other`, a value the neutral model could not represent such
    /// as an HCL template or a TOML datetime, so there is no literal to emit.
    /// The string is the model's noun for it.
    UnrepresentableValue(&'static str),
}

impl Display for EmitError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            EmitError::UnrepresentableName(name) => {
                write!(
                    f,
                    "cannot emit `{name}`: not a valid name in the target format"
                )
            }
            EmitError::UnrepresentableValue(label) => {
                write!(
                    f,
                    "cannot emit {label}: the value has no representation in the model"
                )
            }
        }
    }
}

impl std::error::Error for EmitError {}
