//! The three readers `parse_options` uses for one `#[confval(...)]` key: a
//! bare flag, a `key = PATH` value, and a flag with an optional
//! `(help = "...")` list.

/// Stores a bare flag's own path, kept so a misuse error points at the
/// attribute, or rejects a second one.
pub(super) fn set_flag(
    slot: &mut Option<syn::Path>,
    meta: &syn::meta::ParseNestedMeta<'_>,
    key: &str,
) -> syn::Result<()> {
    if slot.is_some() {
        return Err(meta.error(format!("duplicate confval attribute `{key}`")));
    }
    *slot = Some(meta.path.clone());
    Ok(())
}

/// Stores the path a `key = PATH` attribute names, or rejects a second one.
pub(super) fn set_path<T: syn::parse::Parse>(
    slot: &mut Option<T>,
    meta: &syn::meta::ParseNestedMeta<'_>,
    key: &str,
) -> syn::Result<()> {
    if slot.is_some() {
        return Err(meta.error(format!("duplicate confval attribute `{key}`")));
    }
    *slot = Some(meta.value()?.parse()?);
    Ok(())
}

/// Stores a flag's own path and, when a parenthesized list follows, its
/// `help = "..."` line. Rejects a second flag, the `key = value` form, an
/// unknown key, a non-string help, a blank help, and a second help.
pub(super) fn set_flag_with_help(
    slot: &mut Option<syn::Path>,
    help: &mut Option<syn::LitStr>,
    meta: &syn::meta::ParseNestedMeta<'_>,
    key: &str,
) -> syn::Result<()> {
    if slot.is_some() {
        return Err(meta.error(format!("duplicate confval attribute `{key}`")));
    }
    *slot = Some(meta.path.clone());
    if meta.input.peek(syn::Token![=]) {
        return Err(meta.error(format!(
            "`{key}` takes no value; write `{key}(help = \"...\")`"
        )));
    }
    if !meta.input.peek(syn::token::Paren) {
        return Ok(());
    }
    meta.parse_nested_meta(|inner| {
        if !inner.path.is_ident("help") {
            return Err(inner.error(format!(
                "unknown key in `{key}(...)`; expected `help = \"...\"`"
            )));
        }
        if help.is_some() {
            return Err(inner.error(format!("duplicate `help` in `{key}(...)`")));
        }
        let text: syn::LitStr = inner.value()?.parse()?;
        if text.value().trim().is_empty() {
            return Err(syn::Error::new(
                text.span(),
                format!("`help` in `{key}(...)` must not be empty"),
            ));
        }
        *help = Some(text);
        Ok(())
    })
}
