pub(crate) fn is_ident_has_first_underline(ident: &syn::Ident) -> bool {
    let string = ident.to_string();

    if string.is_empty() {
        return false;
    }

    string.as_bytes()[0] == b'_'
}
