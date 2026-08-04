use confval::source::Located;

#[derive(confval::Spec)]
struct ServerSpec {
    port: Located<i64>,
}

fn to_u16(value: &Located<i64>, _report: &mut confval::diagnostic::Report) -> Option<u16> {
    Some(value.value as u16)
}

#[derive(confval::Config)]
#[confval(lower_from = ServerSpec)]
struct ServerConfig {
    #[confval(lower(from = port, with = to_u16), default)]
    port: u16,
}

fn main() {}
