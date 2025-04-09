use crate::ident_helper::is_ident_has_first_underline;
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned;
use syn::{parse_macro_input, Expr, Ident, Token};

pub(crate) struct SelectInput {
    pub(crate) branches: Vec<Branch>,
    pub(crate) default: Option<Expr>,
}

pub(crate) enum Branch {
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

        while !input.is_empty() {
            let ident: Ident = input.parse().map_err(|_| {
                syn::Error::new(input.span(), "expected `recv`, `send`, or `default`")
            })?;
            if ident == "recv" {
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

                branches.push(Branch::Recv { channel, var, body });
            } else if ident == "send" {
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

                branches.push(Branch::Send {
                    channel,
                    value,
                    var,
                    body,
                });
            } else if ident == "default" {
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

                default = Some(body);
            } else {
                return Err(syn::Error::new(
                    ident.span(),
                    "expected `recv`, `send`, or `default`",
                ));
            }
        }

        Ok(SelectInput { branches, default })
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
    // TODO

    let SelectInput { branches, default } = parse_macro_input!(input as SelectInput);

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
                    let receiver_enum_name = format_ident!("Receiver{idx}");
                    let generic_name = format_ident!("R{idx}");
                    let variant = format_ident!("Receiver{idx}");
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
                    let sender_enum_name = format_ident!("Sender{idx}");
                    let generic_name = format_ident!("S{idx}");
                    let variant = format_ident!("Sender{idx}");
                    let enum_variant = quote! { __SelectReady__::#variant };
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

                    // TODO use mem::forget
                    senders_fn_args.push(quote! { mut #sender_arg_name: Option<#generic_name::Data> });

                    senders_provide_fn_args.push(quote! { Some(#value) });

                    match_arms.push(quote! {
                        #enum_variant(#var) => { #body }
                    });

                    channels_enum_handle.push(quote! {
                        __Channels__::#sender_enum_name(sender) => {
                            match sender.try_send(#sender_arg_name.take().unwrap_unchecked()) {
                                Ok(()) => { return #enum_variant(Ok(())); },
                                Err(TrySendErr::Full(_)) => {},
                                Err(TrySendErr::Locked(v)) => {
                                    channels.push_back(__Channels__::#sender_enum_name(sender));
                                    #sender_arg_name = Some(v);
                                },
                                Err(TrySendErr::Closed(v)) => { return #enum_variant(Err(SendErr::Closed(v))); },
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

        let call_select = if senders_fn_args.is_empty() {
            quote! {__select__(ArrayDeque::from([#(#create_channel_variants),*])) }
        } else {
            quote! {__select__(ArrayDeque::from([#(#create_channel_variants),*]), #(#senders_provide_fn_args),*) }
        };

        let select_signature = if senders_fn_args.is_empty() {
            quote! {
                fn __select__<#(#select_generics),*>(mut channels: ArrayDeque<__Channels__<#(#generics_names),*>, #branches_len>) -> __SelectReady__<#(#generics_names),*>
            }
        } else {
            quote! {
                fn __select__<#(#select_generics),*>(mut channels: ArrayDeque<__Channels__<#(#generics_names),*>, #branches_len>, #(#senders_fn_args),*) -> __SelectReady__<#(#generics_names),*>
            }
        };

        quote! {
            {
                use orengine::sync::channels::{SelectReceiver, SelectSender, TryRecvErr};
                use orengine::sync::{RecvErr, SendErr, TrySendErr};
                use orengine::utils::ArrayDeque;

                enum __SelectReady__<#(#select_generics),*> {
                    #(#select_ready_variants),*
                }

                enum __Channels__<#(#select_generics),*> {
                    #(#channels_enum_variants),*
                }

                // Let the compiler decide whether to inline the function or not.
                #[allow(clippy::too_many_arguments)]
                #select_signature {
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
                    }
                }

                match #call_select {
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
        let mut fn_select_args = Vec::with_capacity(branches.len());
        let mut fn_select_args_types = Vec::with_capacity(branches.len());
        let mut select_calls = Vec::with_capacity(branches.len());
        let mut union_variants = Vec::with_capacity(branches.len());
        let mut union_generics = Vec::with_capacity(branches.len());
        let mut is_local_consts = Vec::with_capacity(branches.len());

        for (idx, branch) in branches.iter().enumerate() {
            let name_of_task_in_select_branch = format_ident!("task_in_select_branch{idx}");
            let create_task_in_select_branch = if idx != branches.len() - 1 {
                quote! {
                    let #name_of_task_in_select_branch = TaskInSelectBranch::new(task_in_select.clone(), #idx);
                }
            } else {
                quote! {
                    let #name_of_task_in_select_branch = unsafe {
                        TaskInSelectBranch::new(task_in_select, #idx)
                    };
                }
            };

            match branch {
                Branch::Recv { channel, var, body } => {
                    let variant = format_ident!("variant{idx}");
                    let generic_name = format_ident!("R{idx}");
                    let receiver_name = format_ident!("receiver_{idx}");

                    fn_select_args.push(quote! {
                        #channel
                    });

                    fn_select_args_types.push(quote! {
                        #receiver_name: &#generic_name
                    });

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

                    match_arms.push(quote! {
                        #idx => {
                            let #var = if !general_state.is_closed() {
                                unsafe { Ok(std::mem::ManuallyDrop::take(&mut recv_slot.#variant)) }
                            } else {
                                Err(RecvErr::Closed)
                            };

                            #body
                        }
                    });

                    select_calls.push(quote! {
                        #create_task_in_select_branch

                        // TODO it can't be AlreadyAcquired when __is_all_local == true
                        match #receiver_name.recv_or_subscribe(
                            recv_slot.cast(),
                            general_state,
                            #name_of_task_in_select_branch,
                        ) {
                            SelectNonBlockingBranchResult::Success => {
                                // `recv_or_subscribe` have already woken the task up
                                // and set the `resolved_branch_id`
                                return;
                            }
                            SelectNonBlockingBranchResult::NotReady => {
                                // Go on, the receiver have been subscribed
                            }
                            SelectNonBlockingBranchResult::AlreadyAcquired => {
                                // Another thread already acquired the lock and wake the task up.
                                return;
                            }
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
                    let sender_name = format_ident!("sender_{idx}");

                    send_vars.push(quote! {
                        let #var_name = #value;
                    });

                    fn_select_args.push(quote! {
                        #channel, &raw const #var_name
                    });

                    fn_select_args_types.push(quote! {
                        #sender_name: &#generic_name, #var_name: *const #generic_name::Data
                    });

                    is_local_consts.push(quote! {
                        orengine::runtime::is_local::<#generic_name>()
                    });

                    generics.push(quote! {
                        #generic_name: SelectSender
                    });

                    match_arms.push(quote! {
                        #idx => {
                            let #var = if !general_state.is_closed() {
                                Ok(())
                            } else {
                                Err(SendErr::Closed(#var_name))
                            };

                            #body
                        }
                    });

                    select_calls.push(quote! {
                        #create_task_in_select_branch

                        // TODO it can't be AlreadyAcquired when __is_all_local == true
                        match #sender_name.send_or_subscribe(
                            unsafe { NonNull::new_unchecked(#var_name.cast_mut()) },
                            general_state,
                            #name_of_task_in_select_branch,
                        ) {
                            SelectNonBlockingBranchResult::Success => {
                                // `send_or_subscribe` have already woken the task up
                                // and set the `resolved_branch_id`
                                return;
                            }
                            SelectNonBlockingBranchResult::NotReady => {
                                // Go on, the receiver have been subscribed
                            }
                            SelectNonBlockingBranchResult::AlreadyAcquired => {
                                // Another thread already acquired the lock and wake the task up.
                                return;
                            }
                        }
                    });
                }
            }
        }

        quote! {
            {
                use std::ptr::NonNull;
                use orengine::local_executor;
                use orengine::utils::SendableNonNull;
                use orengine::sync::channels::waiting_task::{TaskInSelect, TaskInSelectBranch};
                use orengine::sync::channels::select::SelectNonBlockingBranchResult;
                use orengine::sync::channels::{RecvErr, SendErr, SelectReceiver, SelectSender};

                // Task will be woken up when three things are written:
                // 1. `resolved_branch_id` with the id of the branch that has been resolved;
                // 2. `general_state` with `true` if the channel associated with the branch has been closed;
                // 3. `recv_slot` with the value that has been received (or not changed if sent).

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
                    #(#fn_select_args_types),*
                ) {
                    let __is_all_local: bool = #(#is_local_consts) &&*;

                    // We need to check whether `local` task is used in `shared` channel.
                    debug_assert!(
                        !task.is_local() || __is_all_local,
                        "Tried to use `local` task in `select` where at least one channel is `shared`.",
                    );

                    let task_in_select = TaskInSelect::acquire_for_task(task, *resolved_branch_id);

                    unsafe {
                        #(#select_calls)*
                    };

                    // The task is subscribed for all branches. Some of them will wake it up.
                }

                #(#send_vars);*

                let mut recv_slot = __RecvSlot__ { uninit: () };
                let mut resolved_branch_id = usize::MAX;
                // Protected by lock in task in select.
                let mut general_state = orengine::sync::channels::CallState::FirstCall;
                let general_state_ptr = orengine::sync::channels::CallStatePtr::new(&mut general_state);

                let recv_slot_ptr = SendableNonNull::from(&mut recv_slot);
                let resolved_branch_id_ptr = SendableNonNull::from(&mut resolved_branch_id);

                let mut select_closure = |task| {
                    __select__(
                        recv_slot_ptr,
                        resolved_branch_id_ptr,
                        general_state_ptr,
                        task,
                        #(#fn_select_args),*
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

                let __res = match resolved_branch_id {
                    #(#match_arms),*
                    _ => orengine::utils::hints::unreachable_hint()
                };

                __res
            }
        }
    };

    expanded.into()
}

pub(crate) fn select_with_params(input: TokenStream) -> TokenStream {
    // TODO

    let SelectInput { branches, default } = parse_macro_input!(input as SelectInput);

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

    let expanded = if let Some(default_body) = default {
        let mut select_generics = Vec::with_capacity(branches.len());
        let mut generics_names = Vec::with_capacity(branches.len());
        let mut select_ready_variants = Vec::with_capacity(branches.len());
        let mut match_arms = Vec::with_capacity(branches.len());
        let mut select_calls = Vec::with_capacity(branches.len());
        let mut retry_select_calls = Vec::with_capacity(branches.len());
        let mut fn_select_args = Vec::with_capacity(branches.len());
        let mut fn_select_args_types = Vec::with_capacity(branches.len());

        for (idx, branch) in branches.iter().enumerate() {
            match branch {
                Branch::Recv { channel, var, body } => {
                    let receiver_arg_name = format_ident!("receiver{idx}");
                    let generic_name = format_ident!("R{idx}");
                    let variant = format_ident!("Receiver{idx}");
                    let enum_variant = quote! { __SelectReady__::#variant };

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
                        #enum_variant(#var) => { #body }
                    });

                    select_calls.push(quote! {
                        match #receiver_arg_name.try_recv() {
                            Ok(data) => return #enum_variant(Ok(data)),
                            Err(TryRecvErr::Empty) => {},
                            Err(TryRecvErr::Locked) => {
                                number_of_needed_to_retry_branches += 1;
                                locked[#idx] = true;
                            },
                            Err(TryRecvErr::Closed) => return #enum_variant(Err(RecvErr::Closed)),
                        }
                    });

                    retry_select_calls.push(quote! {
                        if locked[#idx] {
                            match #receiver_arg_name.try_recv() {
                                Ok(data) => return #enum_variant(Ok(data)),
                                Err(TryRecvErr::Empty) => {
                                    number_of_needed_to_retry_branches -= 1;
                                    locked[#idx] = false;
                                },
                                Err(TryRecvErr::Locked) => {},
                                Err(TryRecvErr::Closed) => return #enum_variant(Err(RecvErr::Closed)),
                            }
                        }
                    });

                    fn_select_args.push(quote! {
                        #channel
                    });

                    fn_select_args_types.push(quote! {
                        #receiver_arg_name: &#generic_name
                    });
                }

                Branch::Send {
                    channel,
                    value,
                    var,
                    body,
                } => {
                    let sender_arg_name = format_ident!("sender{}", idx);
                    let generic_name = format_ident!("S{}", idx);
                    let variant = format_ident!("Sender{}", idx);
                    let enum_variant = quote! { __SelectReady__::#variant };

                    select_generics.push(quote! {
                        #generic_name: SelectSender
                    });

                    generics_names.push(quote! {
                        #generic_name
                    });

                    select_ready_variants.push(quote! {
                        #variant(Result<(), SendErr<#generic_name::Data>>)
                    });

                    match_arms.push(quote! {
                        #enum_variant(#var) => { #body }
                    });

                    select_calls.push(quote! {
                        match #sender_arg_name.0.try_send(#sender_arg_name.1.take().unwrap_unchecked()) {
                            Ok(()) => return #enum_variant(Ok(())),
                            Err(TrySendErr::Full(_)) => {},
                            Err(TrySendErr::Locked(v)) => {
                                #sender_arg_name.1 = Some(v);
                                number_of_needed_to_retry_branches += 1;
                                locked[#idx] = true;
                            },
                            Err(TrySendErr::Closed(v)) => return #enum_variant(Err(SendErr::Closed(v))),
                        }
                    });

                    retry_select_calls.push(quote! {
                        if locked[#idx] {
                            match #sender_arg_name.0.try_send(#sender_arg_name.1.take().unwrap_unchecked()) {
                                Ok(()) => return #enum_variant(Ok(())),
                                Err(TrySendErr::Full(_)) => {
                                    number_of_needed_to_retry_branches -= 1;
                                    locked[#idx] = false;
                                },
                                Err(TrySendErr::Locked(v)) => {
                                    #sender_arg_name.1 = Some(v);
                                },
                                Err(TrySendErr::Closed(v)) => return #enum_variant(Err(SendErr::Closed(v))),
                            }
                        }
                    });

                    fn_select_args.push(quote! {
                        (#channel, Some(#value))
                    });

                    fn_select_args_types.push(quote! {
                        mut #sender_arg_name: (&#generic_name, Option<#generic_name::Data>)
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

                enum __SelectReady__<#(#select_generics),*> {
                    #(#select_ready_variants),*
                }

                // Let the compiler decide whether to inline the function or not.
                #[allow(clippy::too_many_arguments)]
                fn __select__<#(#select_generics),*>(#(#fn_select_args_types),*) -> __SelectReady__<#(#generics_names),*> {
                    unsafe {
                        #(#select_calls)*
                        // TODO
                    }

                    __SelectReady__::Default
                }

                match __select__(#(#fn_select_args),*) {
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
        let mut fn_select_args = Vec::with_capacity(branches.len());
        let mut fn_select_args_types = Vec::with_capacity(branches.len());
        let mut select_calls = Vec::with_capacity(branches.len());
        let mut union_variants = Vec::with_capacity(branches.len());
        let mut union_generics = Vec::with_capacity(branches.len());
        let mut is_local_consts = Vec::with_capacity(branches.len());

        for (idx, branch) in branches.iter().enumerate() {
            let name_of_task_in_select_branch = format_ident!("task_in_select_branch{idx}");
            let create_task_in_select_branch = if idx != branches.len() - 1 {
                quote! {
                    let #name_of_task_in_select_branch = TaskInSelectBranch::new(task_in_select.clone(), #idx);
                }
            } else {
                quote! {
                    let #name_of_task_in_select_branch = unsafe {
                        TaskInSelectBranch::new(task_in_select, #idx)
                    };
                }
            };

            match branch {
                Branch::Recv { channel, var, body } => {
                    let variant = format_ident!("variant{idx}");
                    let generic_name = format_ident!("R{idx}");
                    let receiver_name = format_ident!("receiver_{idx}");

                    fn_select_args.push(quote! {
                        #channel
                    });

                    fn_select_args_types.push(quote! {
                        #receiver_name: &#generic_name
                    });

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

                    match_arms.push(quote! {
                        #idx => {
                            let #var = if !general_state.is_closed() {
                                unsafe { Ok(std::mem::ManuallyDrop::take(&mut recv_slot.#variant)) }
                            } else {
                                Err(RecvErr::Closed)
                            };

                            #body
                        }
                    });

                    select_calls.push(quote! {
                        #create_task_in_select_branch

                        // TODO it can't be AlreadyAcquired when __is_all_local == true
                        match #receiver_name.recv_or_subscribe(
                            recv_slot.cast(),
                            general_state,
                            #name_of_task_in_select_branch,
                        ) {
                            SelectNonBlockingBranchResult::Success => {
                                // `recv_or_subscribe` have already woken the task up
                                // and set the `resolved_branch_id`
                                return;
                            }
                            SelectNonBlockingBranchResult::NotReady => {
                                // Go on, the receiver have been subscribed
                            }
                            SelectNonBlockingBranchResult::AlreadyAcquired => {
                                // Another thread already acquired the lock and wake the task up.
                                return;
                            }
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
                    let sender_name = format_ident!("sender_{idx}");

                    send_vars.push(quote! {
                        let #var_name = #value;
                    });

                    fn_select_args.push(quote! {
                        #channel, &raw const #var_name
                    });

                    fn_select_args_types.push(quote! {
                        #sender_name: &#generic_name, #var_name: *const #generic_name::Data
                    });

                    is_local_consts.push(quote! {
                        orengine::runtime::is_local::<#generic_name>()
                    });

                    generics.push(quote! {
                        #generic_name: SelectSender
                    });

                    match_arms.push(quote! {
                        #idx => {
                            let #var = if !general_state.is_closed() {
                                Ok(())
                            } else {
                                Err(SendErr::Closed(#var_name))
                            };

                            #body
                        }
                    });

                    select_calls.push(quote! {
                        #create_task_in_select_branch

                        // TODO it can't be AlreadyAcquired when __is_all_local == true
                        match #sender_name.send_or_subscribe(
                            unsafe { NonNull::new_unchecked(#var_name.cast_mut()) },
                            general_state,
                            #name_of_task_in_select_branch,
                        ) {
                            SelectNonBlockingBranchResult::Success => {
                                // `send_or_subscribe` have already woken the task up
                                // and set the `resolved_branch_id`
                                return;
                            }
                            SelectNonBlockingBranchResult::NotReady => {
                                // Go on, the receiver have been subscribed
                            }
                            SelectNonBlockingBranchResult::AlreadyAcquired => {
                                // Another thread already acquired the lock and wake the task up.
                                return;
                            }
                        }
                    });
                }
            }
        }

        quote! {
            {
                use std::ptr::NonNull;
                use orengine::local_executor;
                use orengine::utils::SendableNonNull;
                use orengine::sync::channels::waiting_task::{TaskInSelect, TaskInSelectBranch};
                use orengine::sync::channels::select::SelectNonBlockingBranchResult;
                use orengine::sync::channels::{RecvErr, SendErr, SelectReceiver, SelectSender};

                // Task will be woken up when three things are written:
                // 1. `resolved_branch_id` with the id of the branch that has been resolved;
                // 2. `general_state` with `true` if the channel associated with the branch has been closed;
                // 3. `recv_slot` with the value that has been received (or not changed if sent).

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
                    #(#fn_select_args_types),*
                ) {
                    let __is_all_local: bool = #(#is_local_consts) &&*;

                    // We need to check whether `local` task is used in `shared` channel.
                    debug_assert!(
                        !task.is_local() || __is_all_local,
                        "Tried to use `local` task in `select` where at least one channel is `shared`.",
                    );

                    let task_in_select = TaskInSelect::acquire_for_task(task, *resolved_branch_id);

                    unsafe {
                        #(#select_calls)*
                    };

                    // The task is subscribed for all branches. Some of them will wake it up.
                }

                #(#send_vars);*

                let mut recv_slot = __RecvSlot__ { uninit: () };
                let mut resolved_branch_id = usize::MAX;
                // Protected by lock in task in select.
                let mut general_state = orengine::sync::channels::CallState::FirstCall;
                let general_state_ptr = orengine::sync::channels::CallStatePtr::new(&mut general_state);

                let recv_slot_ptr = SendableNonNull::from(&mut recv_slot);
                let resolved_branch_id_ptr = SendableNonNull::from(&mut resolved_branch_id);

                let mut select_closure = |task| {
                    __select__(
                        recv_slot_ptr,
                        resolved_branch_id_ptr,
                        general_state_ptr,
                        task,
                        #(#fn_select_args),*
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

                let __res = match resolved_branch_id {
                    #(#match_arms),*
                    _ => orengine::utils::hints::unreachable_hint()
                };

                __res
            }
        }
    };

    expanded.into()
}
