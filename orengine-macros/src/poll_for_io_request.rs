use proc_macro::TokenStream;
use quote::quote;
use syn::parse_macro_input;

/// Read docs from `poll_for_io_request`.
pub(crate) fn poll_for_io_request_(input: TokenStream) -> TokenStream {
    let input_elems = parse_macro_input!(input as syn::ExprTuple).elems;

    let do_request = &input_elems[0];
    let ret_statement = &input_elems[1];

    let expanded = quote! {
        if let Some(mut io_request_data) = this.io_request_data.take() {
            match io_request_data.ret() {
                Ok(io_request_data_ret) => {
                    ret = io_request_data_ret;

                    return Poll::Ready(Ok(#ret_statement));
                }
                Err(err) => {
                    return Poll::Ready(Err(err));
                }
            }
        }

        let task = unsafe { orengine::runtime::Task::from_context(cx) };
        this.io_request_data = Some(IoRequestData::new(task));

        #do_request;

        return Poll::Pending;
    };

    TokenStream::from(expanded)
}

/// Read docs from `poll_for_time_bounded_io_request`.
pub(crate) fn poll_for_time_bounded_io_request_(input: TokenStream) -> TokenStream {
    let input_elems = parse_macro_input!(input as syn::ExprTuple).elems;

    let do_request = &input_elems[0];
    let ret_statement = &input_elems[1];

    let expanded = quote! {
        if let Some(mut io_request_data) = this.io_request_data.take() {
            match io_request_data.ret() {
                Ok(io_request_data_ret) => {
                    ret = io_request_data_ret;
                    worker.deregister_time_bounded_io_task(&this.deadline);

                    return Poll::Ready(Ok(#ret_statement));
                }
                Err(err) => {
                    if err.kind() != std::io::ErrorKind::TimedOut {
                        worker.deregister_time_bounded_io_task(&this.deadline);
                    }

                    return Poll::Ready(Err(err));
                }
            }
        }

        let task = unsafe { orengine::runtime::Task::from_context(cx) };
        this.io_request_data = Some(IoRequestData::new(task));

        #do_request;

        return Poll::Pending;
    };

    TokenStream::from(expanded)
}
