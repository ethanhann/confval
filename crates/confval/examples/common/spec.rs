use confval::prelude::*;
use std::collections::BTreeMap;

range_constraint!(PORT, i64, min: 1, max: 65535);
range_constraint!(WORKERS, i64, min: 1, max: 512);
range_constraint!(MAX_BODY_MB, i64, min: 1, max: 1024);
length_constraint!(HOSTNAME_LEN, max: 253);

keyword_enum!(pub LimitMode, {
    Enforce => "enforce",
    Log     => "log",
    Off     => "off",
});

keyword_enum!(pub LogEvent, {
    Request  => "request",
    Response => "response",
    Error    => "error",
});

/// An IPv4 network in CIDR notation, such as `10.0.0.0/8`. A domain format
/// is a consumer type that implements `Format`, the way the built-in `Ip`
/// does, so the derive records and checks it through one attribute.
pub struct Cidr;

impl Format for Cidr {
    const NAME: &'static str = "CIDR block";

    fn check(value: &str) -> bool {
        let Some((address, prefix)) = value.split_once('/') else {
            return false;
        };
        let digits = !prefix.is_empty() && prefix.bytes().all(|b| b.is_ascii_digit());
        address.parse::<std::net::Ipv4Addr>().is_ok()
            && digits
            && prefix.parse::<u8>().is_ok_and(|bits| bits <= 32)
    }
}

#[derive(confval::Spec)]
pub struct ServerSpec {
    #[confval(non_empty, length = HOSTNAME_LEN)]
    pub hostname: Located<String>,
    #[confval(range = PORT)]
    pub port: Located<i64>,
    #[confval(default = 4, range = WORKERS)]
    pub workers: Located<i64>,
    #[confval(default = false)]
    pub tls: Located<bool>,
    // A list field. The bare `default` reads an absent list as empty. Each
    // element keeps its own span, so a bad entry is reported at that entry.
    // `format` on a list records the format each element must parse as, so
    // the derive checks every entry and an empty entry fails with the rest.
    #[confval(default, format = Cidr)]
    pub allow: Vec<Located<String>>,
    // A list whose entries come from a closed set. `keywords` on a list records
    // the set each element must come from, so the derive checks every entry and
    // this field needs no line in `Validate`. The set also reaches the schema,
    // so an editor offers the same words inside the list.
    #[confval(default, keywords = LogEvent)]
    pub log_events: Vec<Located<String>>,
    // An open-ended, string-keyed map. The bare `default` reads an absent map
    // as empty. Each value keeps its span, so a bad entry is reported at that
    // entry, and a key can be any string, including a non-identifier such as a
    // header name.
    #[confval(map, default)]
    pub headers: BTreeMap<String, Located<String>>,
    // Optional in the source. When the block is omitted, the spec keeps it
    // `None`, so a spec dump stays source-faithful. The config side fills the
    // default at lowering time.
    #[confval(nested)]
    pub limits: Option<Located<LimitsSpec>>,
}

#[derive(confval::Spec)]
#[confval(derive_default)]
pub struct LimitsSpec {
    #[confval(default = 16, range = MAX_BODY_MB)]
    pub max_body_mb: Located<i64>,
    #[confval(default = "enforce".to_string(), keywords = LimitMode)]
    pub mode: Located<String>,
}

impl Validate for LimitsSpec {
    // `max_body_mb` and `mode` record their constraints with `#[confval(range)]`
    // and `#[confval(keywords)]`, so the derive checks them. Every rule this
    // block has is recorded, so its `Validate` body is empty.
    fn validate(&self, _report: &mut Report) {}
}

impl Validate for ServerSpec {
    fn validate(&self, report: &mut Report) {
        if self.hostname.value == "0.0.0.0" {
            report
                .warning("hostname set to listen on every available network device")
                .at(self.hostname.span)
                .help("This might be undesired.")
                .emit();
        }

        // A per-entry rule over the map. Each value keeps its span, so an empty
        // header value is reported at the value the operator wrote, not at the
        // whole `headers` field.
        for (name, value) in &self.headers {
            if value.value.is_empty() {
                report
                    .error(format!("header \"{name}\" must not be empty"))
                    .at(value.span)
                    .help("Set the header value, or remove the entry.")
                    .emit();
            }
        }

        // A cross-field rule: the primary span points at the port, and the
        // related span points at the setting that makes the port suspect.
        if self.tls.value && self.port.value == 80 {
            report
                .warning("tls is enabled on port 80, which conventionally serves plaintext HTTP")
                .at(self.port.span)
                .related(self.tls.span, "tls is enabled here")
                .help("Serve TLS on port 443 or another port.")
                .emit();
        }
    }
}
