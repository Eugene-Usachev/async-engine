use crate::ident_helper::is_ident_has_first_underline;
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned;
use syn::{parse_macro_input, Expr, Ident, Token};

struct Deadline {
    deadline: TokenStream,
    body: Expr,
}

struct SelectInput {
    branches: Vec<Branch>,
    default: Option<Expr>,
    deadline: Option<Deadline>,
    // Timeout will be transformed to `deadline`
}

enum Branch {
    Recv {
        channel: Expr,
        var: Ident,
        body: Expr,
    },
    Send {
        channel: Expr,
        value: Expr,
        var: Ident,
        body: Expr,
    },
}

impl Parse for SelectInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut branches = Vec::new();
        let mut default = None;
        let mut deadline = None;

        while !input.is_empty() {
            let fork = input.fork();
            let lookahead: Option<proc_macro2::TokenTree> = fork.parse().ok();
            let found = match lookahead {
                Some(tt) => tt.to_string(),
                None => "end of input".to_string(),
            };

            let ident: Ident = input.parse().map_err(|_| {
                syn::Error::new(
                    input.span(),
                    format!("expected `recv`, `send`, or `default`, found {found}"),
                )
            })?;

            match ident.to_string().as_str() {
                "recv" => {
                    let content;
                    syn::parenthesized!(content in input);

                    let channel: Expr = content.parse().map_err(|_| {
                        syn::Error::new(
                            ident.span(),
                            "expected a channel expression after `recv`. For example, `recv(channel)",
                        )
                    })?;

                    input.parse::<Token![->]>().map_err(|_| {
                        syn::Error::new(
                            channel.span(),
                            "expected `->` after channel expression. \
                         For example, `recv(channel) -> var",
                        )
                    })?;
                    let var: Ident = input.parse().map_err(|_| {
                        syn::Error::new(
                            channel.span(),
                            "expected a variable name after `->`. \
                         For example, `recv(channel) -> var",
                        )
                    })?;

                    input.parse::<Token![=>]>().map_err(|_| {
                        syn::Error::new(
                            var.span(),
                            "expected `=>` after the variable name. \
                         For example, `recv(channel) -> var => body",
                        )
                    })?;
                    let body: Expr = input.parse().map_err(|_| {
                        syn::Error::new(
                            var.span(),
                            "expected an expression after `=>`. \
                         For example, `recv(channel) -> var => { println!(\"received {}!\", var) }`"
                        )
                    })?;

                    // Allow optional comma
                    if input.peek(Token![,]) {
                        let _ = input.parse::<Token![,]>();
                    }

                    branches.push(Branch::Recv { channel, var, body });
                }

                "send" => {
                    let content;
                    syn::parenthesized!(content in input);

                    let channel: Expr = content.parse().map_err(|_| {
                        syn::Error::new(
                            content.span(),
                            "expected a valid channel expression inside `send(...)`",
                        )
                    })?;
                    content.parse::<Token![,]>()?;
                    let value: Expr = content.parse().map_err(|_| {
                        syn::Error::new(
                            content.span(),
                            "expected a valid value expression inside `send(channel, ...)`",
                        )
                    })?;

                    input.parse::<Token![->]>().map_err(|_| {
                        syn::Error::new(
                            value.span(),
                            "expected `->` after value expression. \
                         For example, `send(channel, value) -> result",
                        )
                    })?;
                    let var: Ident = input.parse().map_err(|_| {
                        syn::Error::new(
                            value.span(),
                            "expected a variable name after `->`. \
                         For example, `send(channel, value) -> result`",
                        )
                    })?;

                    input.parse::<Token![=>]>().map_err(|_| {
                        syn::Error::new(
                            var.span(),
                            "expected `=>` after the variable name. \
                         For example, `send(channel, value) -> result => body",
                        )
                    })?;
                    let body: Expr = input.parse().map_err(|_| {
                        syn::Error::new(
                            var.span(),
                            "expected expression after `=>`. \
                         For example, `send(channel, value) -> result => {\
                          if result.is_err() { println!(\"Send operation failed!\")}`",
                        )
                    })?;

                    // Allow optional comma
                    if input.peek(Token![,]) {
                        let _ = input.parse::<Token![,]>();
                    }

                    branches.push(Branch::Send {
                        channel,
                        value,
                        var,
                        body,
                    });
                }

                "deadline" => {
                    if deadline.is_some() {
                        return Err(syn::Error::new(
                            ident.span(),
                            "deadline (timeout) can only be specified once",
                        ));
                    }

                    let deadline_expr: Expr = input.parse().map_err(|_| {
                        syn::Error::new(
                            ident.span(),
                            "expected a deadline expression after `deadline`. \
                         For example, `deadline(deadline)`",
                        )
                    })?;

                    input.parse::<Token![=>]>().map_err(|_| {
                        syn::Error::new(
                            ident.span(),
                            "expected `=>` after `deadline`. \
                         For example, `deadline(deadline) => body`",
                        )
                    })?;

                    let body: Expr = input.parse().map_err(|_| {
                        syn::Error::new(
                            ident.span(),
                            "expected an expression after `=>`. \
                         For example, `deadline(deadline) => { println!(\"Deadline reached!\") }",
                        )
                    })?;

                    // Allow optional comma
                    if input.peek(Token![,]) {
                        let _ = input.parse::<Token![,]>();
                    }

                    deadline = Some(Deadline {
                        deadline: quote! { #deadline_expr }.into(),
                        body,
                    });
                }

                "timeout" => {
                    if deadline.is_some() {
                        return Err(syn::Error::new(
                            ident.span(),
                            "deadline (timeout) can only be specified once",
                        ));
                    }

                    let timeout_expr: Expr = input.parse().map_err(|_| {
                        syn::Error::new(
                            ident.span(),
                            "expected a timeout expression after `timeout`. \
                         For example, `timeout(Duration::from_millis(100))`",
                        )
                    })?;

                    input.parse::<Token![=>]>().map_err(|_| {
                        syn::Error::new(
                            ident.span(),
                            "expected `=>` after `timeout`. \
                         For example, `timeout(Duration::from_millis(100)) => body`",
                        )
                    })?;

                    let body: Expr = input.parse().map_err(|_| {
                        syn::Error::new(
                            ident.span(),
                            "expected an expression after `=>`. \
                         For example, `timeout(Duration::from_millis(100)) => { println!(\"Timeout reached!\") }",
                        )
                    })?;

                    // Allow optional comma
                    if input.peek(Token![,]) {
                        let _ = input.parse::<Token![,]>();
                    }

                    deadline = Some(Deadline {
                        deadline: quote! {
                            orengine::local_executor().start_round_time_for_deadlines() + #timeout_expr
                        }.into(),
                        body,
                    });
                }

                "default" => {
                    input.parse::<Token![=>]>().map_err(|_| {
                        syn::Error::new(
                            ident.span(),
                            "expected `=>` after `default`. \
                         For example, `default => body`",
                        )
                    })?;
                    let body: Expr = input.parse().map_err(|_| {
                        syn::Error::new(
                            ident.span(),
                            "expected an expression after `=>`. \
                         For example, `default => { println!(\"Nothing ready.\")` }",
                        )
                    })?;

                    // Allow optional comma
                    if input.peek(Token![,]) {
                        let _ = input.parse::<Token![,]>();
                    }

                    default = Some(body);
                }

                _ => {
                    return Err(syn::Error::new(
                        input.span(),
                        format!(
                            "expected `recv`, `send`, timeout, deadline or `default`, found {found}"
                        ),
                    ));
                }
            }
        }

        Ok(SelectInput {
            branches,
            default,
            deadline,
        })
    }
}

fn maybe_can_be_simplified(
    len: usize,
    branches: &[Branch],
    default: &Option<Expr>,
) -> Option<TokenStream> {
    if len != 1 {
        return None;
    }

    let expanded = if let Some(default_body) = default {
        match &branches[0] {
            Branch::Recv { channel, var, body } => {
                let var_declaration = if is_ident_has_first_underline(var) {
                    quote! {}
                } else {
                    quote! { let #var; }
                };
                let success_result_initialization = if is_ident_has_first_underline(var) {
                    quote! {}
                } else {
                    quote! { #var = Ok(var); }
                };
                let error_result_initialization = if is_ident_has_first_underline(var) {
                    quote! {}
                } else {
                    quote! { #var = Err(RecvErr::Closed); }
                };

                quote! {
                    {
                        use orengine::sync::{AsyncReceiver, TryRecvErr, RecvErr};

                        let mut __step__ = 0;
                        #var_declaration

                        loop {
                            match (#channel).try_recv() {
                                Ok(var) => { #success_result_initialization },
                                Err(e) => match e {
                                    TryRecvErr::Empty => break #default_body,
                                    TryRecvErr::Closed => { #error_result_initialization },
                                    TryRecvErr::Locked => {
                                        for _ in 0..1 << __step__ {
                                            std::hint::spin_loop();
                                        }

                                        if __step__ <= 6 {
                                            __step__ += 1;
                                        }

                                        continue;
                                    },
                                }
                            }

                            break #body;
                        }
                    }
                }
            }
            Branch::Send {
                channel,
                value,
                var,
                body,
            } => {
                let var_declaration = if is_ident_has_first_underline(var) {
                    quote! {}
                } else {
                    quote! { let #var; }
                };
                let success_result_initialization = if is_ident_has_first_underline(var) {
                    quote! {}
                } else {
                    quote! { #var = Ok(()); }
                };
                let error_result_initialization = if is_ident_has_first_underline(var) {
                    quote! {}
                } else {
                    quote! { #var = Err(RecvErr::Closed(var)); }
                };

                quote! {
                    {
                        use orengine::sync::{AsyncSender, TrySendErr};

                        let mut __step__ = 0;
                        let mut value = #value;
                        #var_declaration

                        loop {
                            match (#channel).try_send(#value) {
                                Ok(()) => { #success_result_initialization },
                                Err(e) => match e {
                                    TrySendErr::Full(_) => break #default_body,
                                    TrySendErr::Closed(var) => { #error_result_initialization },
                                    TrySendErr::Locked(var) => {
                                        value = var;
                                        for _ in 0..1 << __step__ {
                                            std::hint::spin_loop();
                                        }

                                        if __step__ <= 6 {
                                            __step__ += 1;
                                        }

                                        continue;
                                    },
                                }
                            }

                            break #body;
                        }
                    }
                }
            }
        }
    } else {
        match &branches[0] {
            Branch::Recv { channel, var, body } => {
                quote! {
                    {
                        use orengine::sync::{AsyncReceiver, RecvErr};

                        let #var = match (#channel).recv().await {
                            Ok(var) => Ok(var),
                            Err(_) => Err(RecvErr::Closed),
                        };

                        #body
                    }
                }
            }
            Branch::Send {
                channel,
                value,
                var,
                body,
            } => {
                quote! {
                    {
                        use orengine::sync::{AsyncSender, SendErr};

                        let #var = match (#channel).send(#value).await {
                            Ok(()) => Ok(()),
                            Err(SendErr::Closed(var)) => Err(SendErr::Closed(var)),
                        };

                        #body
                    }
                }
            }
        }
    };

    Some(expanded.into())
}

pub(crate) fn select(input: TokenStream, is_sequenced: bool) -> TokenStream {
    let SelectInput {
        branches,
        default,
        deadline,
    } = parse_macro_input!(input as SelectInput);

    if default.is_some() && deadline.is_some() {
        return TokenStream::from(quote! {
            compile_error!("Select cannot have both a default and a deadline (timeout)!");
        });
    }

    let len = branches.len();
    let branches_len = quote! { #len };

    if len == 0 {
        return TokenStream::from(quote! {
            compile_error!("Select must have at least one `recv` or `send` branch!");
        });
    }

    if let Some(simplified_select) = maybe_can_be_simplified(len, &*branches, &default) {
        return simplified_select;
    }

    let shuffle_channels_if_needed = if !is_sequenced {
        quote! { orengine::utils::shuffle(&mut channels); }
    } else {
        quote! {}
    };

    let expanded = if let Some(default_body) = default {
        let mut select_generics = Vec::with_capacity(branches.len());
        let mut generics_names = Vec::with_capacity(branches.len());
        let mut select_ready_variants = Vec::with_capacity(branches.len());
        let mut match_arms = Vec::with_capacity(branches.len());
        let mut senders_fn_args = Vec::with_capacity(branches.len());
        let mut senders_provide_fn_args = Vec::with_capacity(branches.len());

        let mut channels_enum_variants = Vec::with_capacity(branches.len());
        let mut channels_enum_handle = Vec::with_capacity(branches.len());
        let mut create_channel_variants = Vec::with_capacity(branches.len());

        for (idx, branch) in branches.iter().enumerate() {
            match branch {
                Branch::Recv { channel, var, body } => {
                    let generic_name = format_ident!("R{idx}");
                    let variant = format_ident!("Receiver{idx}");
                    let receiver_enum_name = format_ident!("Receiver{idx}");
                    let enum_result_variant = quote! { __SelectReady__::#variant };

                    select_generics.push(quote! {
                        #generic_name: SelectReceiver
                    });

                    select_ready_variants.push(quote! {
                        #variant(Result<#generic_name::Data, RecvErr>)
                    });

                    generics_names.push(quote! {
                        #generic_name
                    });

                    match_arms.push(quote! {
                        #enum_result_variant(#var) => { #body }
                    });

                    channels_enum_handle.push(quote! {
                        __Channels__::#receiver_enum_name(receiver) => {
                            match receiver.try_recv() {
                                Ok(data) => { return #enum_result_variant(Ok(data)); },
                                Err(TryRecvErr::Empty) => {},
                                Err(TryRecvErr::Locked) => {
                                    channels.push_back(__Channels__::#receiver_enum_name(receiver));
                                },
                                Err(TryRecvErr::Closed) => { return #enum_result_variant(Err(RecvErr::Closed)); },
                            }
                        }
                    });

                    channels_enum_variants.push(quote! {
                        #receiver_enum_name(#generic_name)
                    });

                    create_channel_variants.push(quote! {
                        __Channels__::#receiver_enum_name(#channel)
                    });
                }

                Branch::Send {
                    channel,
                    value,
                    var,
                    body,
                } => {
                    let generic_name = format_ident!("S{idx}");
                    let variant = format_ident!("Sender{idx}");
                    let enum_variant = quote! { __SelectReady__::#variant };
                    let sender_enum_name = format_ident!("Sender{idx}");
                    let sender_arg_name = format_ident!("sender_arg{idx}");

                    select_generics.push(quote! {
                        #generic_name: SelectSender
                    });

                    generics_names.push(quote! {
                        #generic_name
                    });

                    select_ready_variants.push(quote! {
                        #variant(Result<(), SendErr<#generic_name::Data>>)
                    });

                    senders_fn_args.push(quote! { #sender_arg_name: #generic_name::Data });

                    senders_provide_fn_args.push(quote! { #value });

                    match_arms.push(quote! {
                        #enum_variant(#var) => { #body }
                    });

                    channels_enum_handle.push(quote! {
                        __Channels__::#sender_enum_name(sender) => {
                            match sender.try_send(std::ptr::read(&#sender_arg_name)) {
                                Ok(()) => {
                                    //we copied it above
                                    std::mem::forget(#sender_arg_name);

                                    return #enum_variant(Ok(()));
                                },
                                Err(TrySendErr::Full(v)) => {
                                    //we copied it above
                                    std::mem::forget(v);
                                },
                                Err(TrySendErr::Locked(v)) => {
                                    //we copied it above
                                    std::mem::forget(v);

                                    channels.push_back(__Channels__::#sender_enum_name(sender));
                                },
                                Err(TrySendErr::Closed(v)) => {
                                    //we copied it above
                                    std::mem::forget(#sender_arg_name);

                                    return #enum_variant(Err(SendErr::Closed(v)));
                                },
                            }
                        }
                    });

                    channels_enum_variants.push(quote! {
                        #sender_enum_name(#generic_name)
                    });

                    create_channel_variants.push(quote! {
                        __Channels__::#sender_enum_name(#channel)
                    });
                }
            }
        }

        select_ready_variants.push(quote! {
            Default,
        });

        quote! {
            {
                use orengine::sync::channels::{SelectReceiver, SelectSender, TryRecvErr};
                use orengine::sync::{RecvErr, SendErr, TrySendErr};
                use orengine::utils::ArrayDeque;

                #[repr(C)]
                enum __SelectReady__<#(#select_generics),*> {
                    #(#select_ready_variants),*
                }

                #[repr(C)]
                enum __Channels__<#(#select_generics),*> {
                    #(#channels_enum_variants),*
                }

                // Let the compiler decide whether to inline the function or not.
                #[allow(clippy::too_many_arguments)]
                fn __select__<#(#select_generics),*>(
                    mut channels: ArrayDeque<__Channels__<#(#generics_names),*>, #branches_len>,
                    #(#senders_fn_args),*
                ) -> __SelectReady__<#(#generics_names),*> {
                    #shuffle_channels_if_needed

                    unsafe {
                        let mut channels_len = #branches_len;
                        loop {
                            for _ in 0..channels_len {
                                match channels.pop_front().unwrap_unchecked() {
                                    #(#channels_enum_handle),*
                                }
                            }

                            if channels.len() == 0 {
                                return __SelectReady__::Default;
                            }

                            for _ in 0..1 << 4 {
                                std::hint::spin_loop();
                                std::hint::spin_loop();
                                std::hint::spin_loop();
                                std::hint::spin_loop();
                            }

                            channels_len = channels.len();
                        }
                    };
                }

                match __select__(ArrayDeque::from([#(#create_channel_variants),*]), #(#senders_provide_fn_args),*)  {
                    __SelectReady__::Default => { #default_body },
                    #(#match_arms),*
                }
            }
        }
    } else {
        let mut send_vars = Vec::with_capacity(branches.len()); // move it to avoid temporary values
        let mut generics = Vec::with_capacity(branches.len());
        let mut union_generic_params = Vec::with_capacity(branches.len());
        let mut match_arms = Vec::with_capacity(branches.len());
        let mut senders_provide_fn_args = Vec::with_capacity(branches.len());
        let mut union_variants = Vec::with_capacity(branches.len());
        let mut union_generics = Vec::with_capacity(branches.len());
        let mut is_local_consts = Vec::with_capacity(branches.len());
        let mut senders_fn_args = Vec::with_capacity(branches.len());
        let mut generics_names = Vec::with_capacity(branches.len());

        let mut channels_enum_variants = Vec::with_capacity(branches.len());
        let mut channels_enum_handle = Vec::with_capacity(branches.len());
        let mut create_channel_variants = Vec::with_capacity(branches.len());
        let mut channels_enum_index_impls = Vec::with_capacity(branches.len());
        let mut idx = 0usize;

        let mut set_deadline_block = quote! {};

        if let Some(deadline) = deadline {
            let deadline_expr = proc_macro2::TokenStream::from(deadline.deadline);
            let deadline_body = deadline.body;

            set_deadline_block = quote! {
                local_executor().register_task_in_select_with_deadline(
                    TaskInSelectBranch::new(task_in_select, 0),
                    #deadline_expr
                );
            };

            match_arms.push(quote! {
                #idx => { #deadline_body }
            });

            idx += 1;
        }

        for branch in branches.iter() {
            match branch {
                Branch::Recv { channel, var, body } => {
                    let variant = format_ident!("variant{idx}");
                    let generic_name = format_ident!("R{idx}");
                    let receiver_enum_name = format_ident!("Receiver{idx}");

                    is_local_consts.push(quote! {
                        orengine::runtime::is_local::<#generic_name>()
                    });

                    union_variants.push(quote! {
                       #variant: std::mem::ManuallyDrop<#generic_name::Data>
                    });

                    union_generics.push(quote! {
                        #generic_name: SelectReceiver
                    });

                    generics.push(quote! {
                        #generic_name: SelectReceiver
                    });

                    union_generic_params.push(quote! {
                        #generic_name
                    });

                    generics_names.push(quote! {
                        #generic_name
                    });

                    match_arms.push(quote! {
                        #idx => {
                            let #var = if !__general_state.is_closed() {
                                unsafe { Ok(std::mem::ManuallyDrop::take(&mut __recv_slot.#variant)) }
                            } else {
                                Err(RecvErr::Closed)
                            };

                            #body
                        }
                    });

                    channels_enum_handle.push(quote! {
                        __Channels__::#receiver_enum_name(receiver) => {
                            match receiver.recv_or_subscribe(
                                recv_slot.cast(),
                                general_state,
                                task_in_select_branch,
                            ) {
                                SelectNonBlockingBranchResult::Success => {
                                    // `recv_or_subscribe` have already woken the task up
                                    // and set the `resolved_branch_id`
                                    return;
                                }
                                SelectNonBlockingBranchResult::NotReady => {
                                    // Go on, the receiver has been subscribed
                                }
                                SelectNonBlockingBranchResult::AlreadyAcquired => {
                                    // Another thread already acquired the lock and waked the task up.
                                    return;
                                }
                            }
                        }
                    });

                    channels_enum_variants.push(quote! {
                        #receiver_enum_name(#generic_name)
                    });

                    create_channel_variants.push(quote! {
                        __Channels__::#receiver_enum_name(#channel)
                    });

                    channels_enum_index_impls.push(quote! {
                        __Channels__::#receiver_enum_name(_) => {
                            #idx
                        }
                    });
                }

                Branch::Send {
                    channel,
                    value,
                    var,
                    body,
                } => {
                    let generic_name = format_ident!("S{idx}");
                    let var_name = format_ident!("__data{idx}");
                    let sender_enum_name = format_ident!("Sender{idx}");

                    send_vars.push(quote! {
                        let #var_name = #value;
                    });

                    senders_provide_fn_args.push(quote! {
                        &raw const #var_name
                    });

                    senders_fn_args.push(quote! {
                        #var_name: *const #generic_name::Data
                    });

                    is_local_consts.push(quote! {
                        orengine::runtime::is_local::<#generic_name>()
                    });

                    generics.push(quote! {
                        #generic_name: SelectSender
                    });

                    generics_names.push(quote! {
                        #generic_name
                    });

                    match_arms.push(quote! {
                        #idx => {
                            let #var = if !__general_state.is_closed() {
                                std::mem::forget(#var_name);

                                Ok(())
                            } else {
                                Err(SendErr::Closed(#var_name))
                            };

                            #body
                        }
                    });

                    channels_enum_handle.push(quote! {
                        __Channels__::#sender_enum_name(sender) => {
                            match sender.send_or_subscribe(
                                NonNull::new_unchecked(#var_name.cast_mut()),
                                general_state,
                                task_in_select_branch,
                            ) {
                                SelectNonBlockingBranchResult::Success => {
                                    // `send_or_subscribe` have already woken the task up
                                    // and set the `resolved_branch_id`
                                    return;
                                }
                                SelectNonBlockingBranchResult::NotReady => {
                                    // Go on, the receiver has been subscribed
                                }
                                SelectNonBlockingBranchResult::AlreadyAcquired => {
                                    // Another thread already acquired the lock and waked the task up.
                                    return;
                                }
                            }
                        }
                    });

                    channels_enum_variants.push(quote! {
                        #sender_enum_name(#generic_name)
                    });

                    create_channel_variants.push(quote! {
                        __Channels__::#sender_enum_name(#channel)
                    });

                    channels_enum_index_impls.push(quote! {
                        __Channels__::#sender_enum_name(_) => {
                            #idx
                        }
                    });
                }
            }

            idx += 1;
        }

        quote! {
            {
                use std::ptr::NonNull;
                use orengine::local_executor;
                use orengine::utils::SendableNonNull;
                use orengine::sync::channels::waiting_task::{TaskInSelect, TaskInSelectBranch};
                use orengine::sync::channels::select::SelectNonBlockingBranchResult;
                use orengine::sync::channels::{RecvErr, SendErr, SelectReceiver, SelectSender};

                #[repr(C)]
                enum __Channels__<#(#generics),*> {
                    #(#channels_enum_variants),*
                }

                impl<#(#generics),*> __Channels__<#(#generics_names),*> {
                    fn index(&self) -> usize {
                        match self {
                            #(#channels_enum_index_impls),*
                        }
                    }
                }

                // Task will be woken up when three things are written:
                // 1. `resolved_branch_id` with the id of the branch that has been resolved;
                // 2. `general_state` with `true` if the channel associated with the branch has been closed;
                // 3. `recv_slot` with the value that has been received (or not changed if sent).

                #[repr(C)]
                union __RecvSlot__<#(#union_generics),*> {
                    uninit: (),
                    #(#union_variants),*
                }

                unsafe impl<#(#union_generics),*> Send for __RecvSlot__<#(#union_generic_params),*> {}

                #[allow(clippy::too_many_arguments)]
                fn __select__<#(#generics),*>(
                    recv_slot: SendableNonNull<__RecvSlot__<#(#union_generic_params),*>>,
                    resolved_branch_id: SendableNonNull<usize>,
                    general_state: orengine::sync::channels::CallStatePtr,
                    task: orengine::runtime::Task,
                    mut channels: [__Channels__<#(#generics_names),*>; #branches_len],
                    #(#senders_fn_args),*
                ) {
                    let __is_all_local: bool = #(#is_local_consts) &&*;

                    // We need to check whether `local` task is used in `shared` channel.
                    debug_assert!(
                        !task.is_local() || __is_all_local,
                        "Tried to use `local` task in `select` where at least one channel is `shared`.",
                    );

                    #shuffle_channels_if_needed

                    let task_in_select = TaskInSelect::acquire_for_task(task, *resolved_branch_id);

                    unsafe {
                        for i in 0..#branches_len {
                            let chan_ref = channels.get_unchecked_mut(i);
                            let task_in_select_branch = TaskInSelectBranch::new(task_in_select.clone(), chan_ref.index());

                            match chan_ref {
                                #(#channels_enum_handle),*
                            }
                        }
                    };

                    #set_deadline_block

                    // The task is subscribed for all branches. Some of them will wake it up.
                }

                #(#send_vars)*

                let mut __recv_slot = __RecvSlot__ { uninit: () };
                let mut __resolved_branch_id = usize::MAX;
                // Protected by lock in the task in select.
                let mut __general_state = orengine::sync::channels::CallState::FirstCall;
                let __general_state_ptr = orengine::sync::channels::CallStatePtr::new(&mut __general_state);

                let __recv_slot_ptr = SendableNonNull::from(&mut __recv_slot);
                let __resolved_branch_id_ptr = SendableNonNull::from(&mut __resolved_branch_id);

                let mut select_closure = |task| {
                    __select__(
                        __recv_slot_ptr,
                        __resolved_branch_id_ptr,
                        __general_state_ptr,
                        task,
                        [#(#create_channel_variants),*],
                        #(#senders_provide_fn_args),*
                    );
                };

                unsafe {
                    local_executor()
                        .invoke_call(
                            orengine::runtime::Call::call_fn(
                                std::mem::transmute::<
                                    &mut dyn FnMut(orengine::runtime::Task),
                                    *mut dyn FnMut(orengine::runtime::Task)
                                >(&mut select_closure)
                            ),
                        );
                    orengine::runtime::Task::park_current_task().await;
                };

                // Task is unparked here. So, we can read the result

                let __res = match __resolved_branch_id {
                    #(#match_arms),*
                    _ => orengine::utils::hints::unreachable_hint()
                };

                __res
            }
        }
    };

    expanded.into()
}
