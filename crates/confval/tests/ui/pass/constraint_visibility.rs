//! `range_constraint!` and `length_constraint!` pass test.
//!
//! This pins the attribute slot and the visibility slot of both macros from
//! outside their defining module. Every public form of each macro appears
//! with `pub`, with `pub(crate)`, and with no visibility, and one constant
//! uses `pub(in path)`. The crate denies `dead_code`. An unused `pub` constant
//! then compiles only when its `#[allow(dead_code)]` attaches to the emitted
//! const.

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
        pub DRAIN, i64, min: 0, max: 300, units: "seconds"
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

    range_constraint!(pub(crate) CRATE_PORT, i64, min: 1, max: 65535);
    range_constraint!(pub(crate) CRATE_DRAIN, i64, min: 0, max: 300, units: "seconds");
    range_constraint!(pub(crate) CRATE_WORKERS, i64, min: 1, max: 512, help: "Match this to your CPU core count.");
    range_constraint!(pub(crate) CRATE_TIMEOUT, i64, min: 1, max: 300, units: "seconds", help: "Keep this under 5 minutes.");

    range_constraint!(
        /// A bound visible to the `bounds` module tree only.
        #[allow(dead_code)]
        pub(in crate::bounds) INNER, u8, min: 1, max: 2
    );

    length_constraint!(
        /// The hostname bound.
        pub HOSTNAME_LEN, max: 253
    );
    length_constraint!(
        /// The label bound.
        pub LABEL_LEN, min: 1, max: 63
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

    length_constraint!(pub(crate) CRATE_HOSTNAME_LEN, max: 253);
    length_constraint!(pub(crate) CRATE_LABEL_LEN, min: 1, max: 63);
    length_constraint!(pub(crate) CRATE_NAME_LEN, max: 63, help: "Keep names short.");
    length_constraint!(pub(crate) CRATE_PATH_LEN, min: 1, max: 4096, help: "A path is at most 4096 characters.");

    pub(crate) fn crate_sums() -> (i64, usize) {
        let range = CRATE_PORT.max + CRATE_DRAIN.max + CRATE_WORKERS.max + CRATE_TIMEOUT.max;
        let length =
            CRATE_HOSTNAME_LEN.max + CRATE_LABEL_LEN.max + CRATE_NAME_LEN.max + CRATE_PATH_LEN.max;
        (range, length)
    }
}

range_constraint!(PRIVATE_PORT, u16, min: 1, max: 65535);
range_constraint!(PRIVATE_DRAIN, i64, min: 0, max: 300, units: "seconds");
range_constraint!(PRIVATE_WORKERS, i64, min: 1, max: 512, help: "Match this to your CPU core count.");
range_constraint!(PRIVATE_TIMEOUT, i64, min: 1, max: 300, units: "seconds", help: "Keep this under 5 minutes.");

length_constraint!(PRIVATE_HOSTNAME_LEN, max: 253);
length_constraint!(PRIVATE_LABEL_LEN, min: 0, max: 8);
length_constraint!(PRIVATE_NAME_LEN, max: 63, help: "Keep names short.");
length_constraint!(PRIVATE_PATH_LEN, min: 1, max: 4096, help: "A path is at most 4096 characters.");

fn main() {
    assert_eq!((bounds::PORT.min, bounds::PORT.max), (1, 65535));
    assert_eq!(bounds::DRAIN.units, Some("seconds"));
    assert_eq!((bounds::HOSTNAME_LEN.min, bounds::HOSTNAME_LEN.max), (0, 253));
    assert_eq!((bounds::LABEL_LEN.min, bounds::LABEL_LEN.max), (1, 63));
    assert_eq!(bounds::crate_sums(), (65535 + 300 + 512 + 300, 253 + 63 + 63 + 4096));
    assert_eq!(PRIVATE_PORT.max, 65535);
    assert_eq!(PRIVATE_DRAIN.units, Some("seconds"));
    assert_eq!(PRIVATE_WORKERS.help, Some("Match this to your CPU core count."));
    assert_eq!(PRIVATE_TIMEOUT.units, Some("seconds"));
    assert_eq!(PRIVATE_HOSTNAME_LEN.max, 253);
    assert_eq!(PRIVATE_LABEL_LEN.max, 8);
    assert_eq!(PRIVATE_NAME_LEN.help, Some("Keep names short."));
    assert_eq!(PRIVATE_PATH_LEN.min, 1);
}
