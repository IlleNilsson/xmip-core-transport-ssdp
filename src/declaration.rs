//! This technology's declaration: the transport guide's "The declaration",
//! `module/core/capability/transport/doc/adding-a-transport.md`.

use xcore::settings::{Applies, Kind, Presence, Setting, Settings};

/// Every setting a Location takes beyond its address, which is where a
/// Receive Location listens: the group `239.255.255.250:1900`, or a unicast
/// address for answers to its own searches. A send's target is each send's
/// own.
pub const SETTINGS: &Settings = &Settings {
    technology: env!("CARGO_PKG_NAME"),
    settings: &[
        Setting {
            name: "st",
            kind: Kind::Text,
            presence: Presence::Optional,
            meaning: "The search target this node announces and answers searches for, given \
                      with `usn` and `location`; nothing is announced when left out.",
            applies: Applies::Both,
        },
        Setting {
            name: "usn",
            kind: Kind::Text,
            presence: Presence::Optional,
            meaning: "The unique service name this node announces, given with `st` and \
                      `location`.",
            applies: Applies::Both,
        },
        Setting {
            name: "location",
            kind: Kind::Text,
            presence: Presence::Optional,
            meaning: "The URL of the description this node announces, given with `st` and \
                      `usn`.",
            applies: Applies::Both,
        },
        transport::configured::timeout(
            Applies::Receive,
            None,
            "How long a receive waits for a message; unbounded when left out.",
        ),
    ],
};

/// This technology's schemes, as [`transport::Configured::SCHEMES`] defines
/// them.
pub const SCHEMES: &[&str] = &["ssdp"];
