use proc_macro::TokenStream;
use quote::quote;
use syn::parse_macro_input;

pub(crate) fn parse_args_to_timeout(input: TokenStream) -> Option<usize> {
    let args_string = input.to_string();

    const TIMEOUT_PREFIXES: [&str; 3] = ["timeout_ms = ", "timeout_ms=", "timeout_ms ="];

    for prefix in TIMEOUT_PREFIXES {
        if args_string.starts_with(prefix) {
            if let Some(timeout_str) = args_string.strip_prefix(prefix) {
                return if let Ok(timeout_num) = timeout_str.parse::<usize>() {
                    Some(timeout_num)
                } else {
                    panic!("Timeout value must be a number!")
                };
            }
        }
    }

    None
}

/// Generates a test function with a provided locality.
pub(crate) fn generate_test(
    input: TokenStream,
    is_local: bool,
    timeout: Option<usize>,
) -> TokenStream {
    let fn_item = parse_macro_input!(input as syn::ItemFn);
    let body = &fn_item.block;
    let attrs = &fn_item.attrs;
    let signature = &fn_item.sig;
    let name = &signature.ident;
    let name_str = name.to_string();

    assert!(
        signature.inputs.is_empty(),
        "Test function must have zero arguments!"
    );

    let spawn_fn = if is_local {
        quote! { orengine::test::run_test_and_block_on_local }
    } else {
        quote! { orengine::test::run_test_and_block_on_shared }
    };

    let timeout = if let Some(timeout) = timeout {
        quote! { Some(std::time::Duration::from_millis(#timeout as u64)) }
    } else {
        quote! { None }
    };

    let expanded = quote! {
        #[test]
        #(#attrs)*
        fn #name() {
            println!("Test {} started!", #name_str.to_string());

            #spawn_fn(|| async {
                #body
            }, #timeout);

            println!("Test {} finished!", #name_str.to_string());
        }
    };

    TokenStream::from(expanded)
}
