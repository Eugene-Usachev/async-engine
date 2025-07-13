use proc_macro::TokenStream;
use quote::quote;
use syn::parse_macro_input;

pub(crate) fn parse_test_args(input: TokenStream) -> (Option<usize>, Option<String>) {
    let args_string = input.to_string();
    let mut timeout = None;
    let mut exclusive_in = None;

    // Parse timeout_ms
    const TIMEOUT_PREFIXES: [&str; 3] = ["timeout_ms = ", "timeout_ms=", "timeout_ms ="];
    for prefix in TIMEOUT_PREFIXES {
        if args_string.starts_with(prefix) {
            if let Some(timeout_str) = args_string
                .split(',')
                .next()
                .and_then(|s| s.strip_prefix(prefix))
            {
                timeout = if let Ok(timeout_num) = timeout_str.parse::<usize>() {
                    Some(timeout_num)
                } else {
                    panic!("Timeout value must be a number!")
                };
                break;
            }
        }
    }

    // Parse exclusive_in
    const EXCLUSIVE_PREFIXES: [&str; 3] = ["exclusive_in = ", "exclusive_in=", "exclusive_in ="];
    for prefix in EXCLUSIVE_PREFIXES {
        if args_string.contains(prefix) {
            if let Some(exclusive_str) = args_string
                .split(',')
                .find(|s| s.trim().starts_with("exclusive_in"))
            {
                if let Some(value) = exclusive_str.split('=').nth(1) {
                    let value = value.trim().trim_matches('"').trim();
                    exclusive_in = Some(value.to_string());
                }
            }
            break;
        }
    }

    (timeout, exclusive_in)
}

/// Generates a test function with a provided locality.
pub(crate) fn generate_test(
    input: TokenStream,
    is_local: bool,
    timeout: Option<usize>,
    exclusive_in: Option<String>,
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

    let exclusive_in = if let Some(exclusive_in) = exclusive_in {
        quote! { Some(#exclusive_in.to_string()) }
    } else {
        quote! { None }
    };

    let expanded = quote! {
        #[test]
        #(#attrs)*
        fn #name() {
            #spawn_fn(|| async {
                println!("Test {} started!", #name_str.to_string());

                #body
            }, #timeout, #exclusive_in);

            println!("Test {} finished!", #name_str.to_string());
        }
    };

    TokenStream::from(expanded)
}
