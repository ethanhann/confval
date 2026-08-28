//! The bounds the spec records. Each one is `pub(crate)`. The spec module
//! names it in a recording attribute.

use confval::{length_constraint, range_constraint};

range_constraint!(
    /// The listening port.
    pub(crate) PORT, i64, min: 1, max: 65535
);
range_constraint!(
    /// The worker count.
    pub(crate) WORKERS, i64, min: 1, max: 512
);
range_constraint!(
    /// The request body cap in megabytes.
    pub(crate) MAX_BODY_MB, i64, min: 1, max: 1024
);
length_constraint!(
    /// The hostname bound, one DNS name.
    pub(crate) HOSTNAME_LEN, max: 253
);
