//! `range_constraint!` and `length_constraint!` pass test.
//!
//! This pins the attribute slot and the visibility slot of both macros from
//! outside their defining module. Every public form of each macro compiles
//! with `pub`, `pub(crate)`, and no visibility. The crate denies `dead_code`.
//! An unused `pub` constant then compiles only when its `#[allow(dead_code)]`
//! attaches to the emitted const.

#![deny(dead_code)]

use confval::{length_constraint, range_constraint};

mod bounds {
    use confval::{length_constraint, range_constraint};

    range_constraint!(
        /// The listening port.
        pub PORT, i64, min: 1, max: 65535
    );
    range_constraint!(
        /// The drain window.
        pub(crate) DRAIN, i64, min: 0, max: 300, units: "seconds"
    );
    range_constraint!(
        /// The worker count.
        #[allow(dead_code)]
        pub WORKERS, i64, min: 1, max: 512, help: "Match this to your CPU core count."
    );
    range_constraint!(
        /// The shutdown timeout.
        #[allow(dead_code)]
        pub TIMEOUT, i64, min: 1, max: 300, units: "seconds", help: "Keep this under 5 minutes."
    );

    length_constraint!(
        /// The hostname bound.
        pub HOSTNAME_LEN, max: 253
    );
    length_constraint!(
        /// The label bound.
        pub(crate) LABEL_LEN, min: 1, max: 63
    );
    length_constraint!(
        /// The name bound.
        #[allow(dead_code)]
        pub NAME_LEN, max: 63, help: "Keep names short."
    );
    length_constraint!(
        /// The path bound.
        #[allow(dead_code)]
        pub PATH_LEN, min: 1, max: 4096, help: "A path is at most 4096 characters."
    );
}

range_constraint!(PRIVATE_PORT, u16, min: 1, max: 65535);
length_constraint!(PRIVATE_LEN, min: 0, max: 8);

fn main() {
    assert_eq!((bounds::PORT.min, bounds::PORT.max), (1, 65535));
    assert_eq!(bounds::DRAIN.units, Some("seconds"));
    assert_eq!((bounds::HOSTNAME_LEN.min, bounds::HOSTNAME_LEN.max), (0, 253));
    assert_eq!((bounds::LABEL_LEN.min, bounds::LABEL_LEN.max), (1, 63));
    assert_eq!(PRIVATE_PORT.max, 65535);
    assert_eq!(PRIVATE_LEN.max, 8);
}
