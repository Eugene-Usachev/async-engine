//! This crate provides tools to generate code with using `orengine` crate.
#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![deny(clippy::blanket_clippy_restriction_lints)]
#![warn(clippy::nursery)]
#![allow(clippy::implicit_return)]
#![allow(clippy::indexing_slicing)]
#![allow(clippy::panic)]

extern crate proc_macro;
mod select;

use crate::select::SelectInput;
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::parse_macro_input;

/// Generates code for [`Future::poll`](std::future::Future::poll).
///
/// # Must have above
///
/// * `this` with `io_request_data` (`Option<IoRequestData>`) fields;
///
/// * `cx` with `waker` method that returns ([`Waker`](std::task::Waker)) which contains
///   `*const orengine::runtime::Task` in `data` field;
///
/// * declared variable `ret` (`usize`) which can be used in `ret_statement`.
///
/// # Arguments
///
/// * `do_request` - the request code
/// * `ret_statement` - the return statement which can use `ret`(`usize`) variable.
#[proc_macro]
pub fn poll_for_io_request(input: TokenStream) -> TokenStream {
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

/// Generates code for [`Future::poll`](std::future::Future::poll).
///
/// # The difference between `poll_for_io_request` and `poll_for_time_bounded_io_request`
///
/// `poll_for_time_bounded_io_request` deregisters the time bounded task after execution.
///
/// # Must have above
///
/// * `this` with `io_request_data` (`Option<IoRequestData>`) and `deadline`
///   ([`Instant`](std::time::Instant)) fields;
///
/// * `cx` with `waker` method that returns ([`Waker`](std::task::Waker)) which contains
///   `*const orengine::runtime::Task` in `data` field;
///
/// * declared variable `ret` (`usize`) which can be used in `ret_statement`;
///
/// * `worker` from `local_worker()`.
///
/// # Arguments
///
/// * `do_request` - the request code
/// * `ret_statement` - the return statement which can use `ret`(`usize`) variable.
#[proc_macro]
pub fn poll_for_time_bounded_io_request(input: TokenStream) -> TokenStream {
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

/// Generates a test function with provided locality.
fn generate_test(input: TokenStream, is_local: bool) -> TokenStream {
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

    let expanded = quote! {
        #[test]
        #(#attrs)*
        fn #name() {
            println!("Test {} started!", #name_str.to_string());
            #spawn_fn(async {
                #body
            });
            println!("Test {} finished!", #name_str.to_string());
        }
    };

    TokenStream::from(expanded)
}

/// Generates a test function with running an `Executor` with `local` task.
///
/// # The difference between `test_local` and [`test_shared()`]
///
/// `test_local` generates a test function that runs an `Executor` with `local` task.
/// [`test_shared()`] generates a test function that runs an
/// `Executor` with `shared` task.
///
/// # Example
///
/// ```ignore
/// #[orengine::test::test_local]
/// fn test_sleep() {
///     let start = std::time::Instant::now();
///     orengine::sleep(std::time::Duration::from_secs(1)).await;
///     assert!(start.elapsed() >= std::time::Duration::from_secs(1));
/// }
/// ```
///
/// # Note
///
/// Code above is equal to:
///
/// ```ignore
/// #[test]
/// fn test_sleep() {
///     println!("Test sleep started!");
///     orengine::test::run_test_and_block_on_local(async {
///         let start = std::time::Instant::now();
///         orengine::sleep(std::time::Duration::from_secs(1)).await;
///         assert!(start.elapsed() >= std::time::Duration::from_secs(1));
///     });
///     println!("Test sleep finished!");
/// }
/// ```
#[proc_macro_attribute]
pub fn test_local(_: TokenStream, input: TokenStream) -> TokenStream {
    generate_test(input, true)
}

/// Generates a test function with running an `Executor` with `local` task.
///
/// # The difference between `test_shared` and [`test_local()`]
///
/// [`test_shared()`] generates a test function that runs an `Executor` with `shared` task.
/// `test_local` generates a test function that runs an `Executor` with `local` task.
///
/// # Example
///
/// ```ignore
/// #[orengine::test::test_shared]
/// fn test_sleep() {
///     let start = std::time::Instant::now();
///     orengine::sleep(std::time::Duration::from_secs(1)).await;
///     assert!(start.elapsed() >= std::time::Duration::from_secs(1));
/// }
/// ```
///
/// # Note
///
/// Code above is equal to:
///
/// ```ignore
/// #[test]
/// fn test_sleep() {
///     println!("Test sleep started!");
///     orengine::test::run_test_and_block_on_shared(async {
///         let start = std::time::Instant::now();
///         orengine::sleep(std::time::Duration::from_secs(1)).await;
///         assert!(start.elapsed() >= std::time::Duration::from_secs(1));
///     });
///     println!("Test sleep finished!");
/// }
/// ```
#[proc_macro_attribute]
pub fn test_shared(_: TokenStream, input: TokenStream) -> TokenStream {
    generate_test(input, false)
}

#[proc_macro]
pub fn select(input: TokenStream) -> TokenStream {
    let SelectInput { branches, default } = parse_macro_input!(input as SelectInput);

    let len = branches.len();
    let branches_len = quote! { #len };

    if len == 0 {
        return TokenStream::from(quote! {
            compile_error!("Select must have at least one `recv` or `send` branch!");
        });
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
                select::Branch::Recv { channel, var, body } => {
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

                select::Branch::Send {
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
                    let mut locked = [false; #branches_len];
                    let mut number_of_needed_to_retry_branches = 0;

                    unsafe {
                        #(#select_calls)*

                        loop {
                            if number_of_needed_to_retry_branches == 0 {
                                break;
                            }

                            #(#retry_select_calls)*
                        }
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
        // TODO maybe Call::select with accepting args?

        let mut send_vars = Vec::with_capacity(branches.len()); // move it to avoid temporary values
        let mut generics = Vec::with_capacity(branches.len());
        let mut union_generic_params = Vec::with_capacity(branches.len());
        let mut match_arms = Vec::with_capacity(branches.len());
        let mut fn_select_args = Vec::with_capacity(branches.len());
        let mut fn_select_args_types = Vec::with_capacity(branches.len());
        let mut select_calls = Vec::with_capacity(branches.len());
        let mut retry_select_calls = Vec::with_capacity(branches.len());
        let mut union_variants = Vec::with_capacity(branches.len());
        let mut union_generics = Vec::with_capacity(branches.len());
        let mut is_local_consts = Vec::with_capacity(branches.len());

        for (idx, branch) in branches.iter().enumerate() {
            let name_of_task_in_select_branch = format_ident!("task_in_select_branch{idx}");

            match branch {
                select::Branch::Recv { channel, var, body } => {
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
                            let #var = if unsafe { !general_state.as_recv_and_is_closed() } {
                                unsafe { Ok(std::mem::ManuallyDrop::take(&mut recv_slot.#variant)) }
                            } else {
                                Err(RecvErr::Closed)
                            };

                            #body
                        }
                    });

                    select_calls.push(quote! {
                        let mut #name_of_task_in_select_branch = if __is_all_local {
                            unsafe { TaskInSelectBranch::new_local(task_in_select, #idx) }
                        } else {
                            TaskInSelectBranch::new(task_in_select, #idx)
                        };

                        // TODO it can't be AlreadyAcquired when __is_all_local == true
                        match #receiver_name.recv_or_subscribe(
                            recv_slot.cast(),
                            general_state,
                            #name_of_task_in_select_branch,
                            __is_all_local
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
                            SelectNonBlockingBranchResult::Locked => {
                                number_of_needed_to_retry_branches += 1;
                                locked[#idx] = true;
                            }
                        }
                    });

                    retry_select_calls.push(quote! {
                        if locked[#idx] {
                             // TODO it can't be AlreadyAcquired when __is_all_local == true
                            match #receiver_name.recv_or_subscribe(
                                recv_slot.cast(),
                                general_state,
                                #name_of_task_in_select_branch,
                                __is_all_local
                            ) {
                                SelectNonBlockingBranchResult::Success => {
                                    // `recv_or_subscribe` have already woken the task up
                                    // and set the `resolved_branch_id`
                                    return;
                                }
                                SelectNonBlockingBranchResult::NotReady => {
                                    number_of_needed_to_retry_branches -= 1;
                                    locked[#idx] = false;
                                }
                                SelectNonBlockingBranchResult::AlreadyAcquired => {
                                    // Another thread already acquired the lock and wake the task up.
                                    return;
                                }
                                SelectNonBlockingBranchResult::Locked => {}
                            }
                        }
                    });
                }

                select::Branch::Send {
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
                            let #var = if unsafe { !general_state.as_recv_and_is_closed() } {
                                Ok(())
                            } else {
                                Err(SendErr::Closed(#var_name))
                            };

                            #body
                        }
                    });

                    select_calls.push(quote! {
                        let mut #name_of_task_in_select_branch = if __is_all_local {
                            unsafe { TaskInSelectBranch::new_local(task_in_select, #idx) }
                        } else {
                            TaskInSelectBranch::new(task_in_select, #idx)
                        };

                        // TODO it can't be AlreadyAcquired when __is_all_local == true
                        match #sender_name.send_or_subscribe(
                            unsafe { NonNull::new_unchecked(#var_name.cast_mut()) },
                            general_state,
                            #name_of_task_in_select_branch,
                            __is_all_local
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
                            SelectNonBlockingBranchResult::Locked => {
                                number_of_needed_to_retry_branches += 1;
                                locked[#idx] = true;
                            }
                        }
                    });

                    retry_select_calls.push(quote! {
                        if locked[#idx] {
                            // TODO it can't be AlreadyAcquired when __is_all_local == true
                            match #sender_name.send_or_subscribe(
                                unsafe { NonNull::new_unchecked(#var_name.cast_mut()) },
                                general_state,
                                #name_of_task_in_select_branch,
                                __is_all_local
                            ) {
                                SelectNonBlockingBranchResult::Success => {
                                    // `send_or_subscribe` have already woken the task up
                                    // and set the `resolved_branch_id`
                                    return;
                                }
                                SelectNonBlockingBranchResult::NotReady => {
                                    number_of_needed_to_retry_branches -= 1;
                                    locked[#idx] = false;
                                }
                                SelectNonBlockingBranchResult::AlreadyAcquired => {
                                    // Another thread already acquired the lock and wake the task up.
                                    return;
                                }
                                SelectNonBlockingBranchResult::Locked => {}
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

                #[allow(clippy::too_many_arguments)]
                fn __select__<#(#generics),*>(
                    recv_slot: NonNull<__RecvSlot__<#(#union_generic_params),*>>,
                    resolved_branch_id: NonNull<usize>,
                    general_state: orengine::sync::channels::states::PtrToCallState,
                    task: orengine::runtime::Task,
                    #(#fn_select_args_types),*
                ) {
                    let __is_all_local: bool = #(#is_local_consts) &&*;

                    // We need to check whether `local` task is used in `shared` channel.
                    debug_assert!(
                        !task.is_local() || __is_all_local,
                        "Tried to use `local` task in `select` where at least one channel is `shared`.",
                    );

                    let task_in_select = unsafe { TaskInSelect::acquire_for_task_with_lock(task, resolved_branch_id) };
                    let mut locked = [false; #branches_len];
                    let mut number_of_needed_to_retry_branches = 0;

                    unsafe {
                        #(#select_calls)*

                        loop {
                            if number_of_needed_to_retry_branches == 0 {
                                break;
                            }

                            #(#retry_select_calls)*
                        }
                    }

                    // The task is subscribed for all branches. Some of them will wake it up.
                }

                #(#send_vars);*

                let mut recv_slot = __RecvSlot__ { uninit: () };
                let mut resolved_branch_id = usize::MAX;
                // Protected by lock in task in select.
                let mut general_state = orengine::sync::channels::states::PtrToCallState::uninit();

                let recv_slot_ptr = NonNull::from(&mut recv_slot);
                let resolved_branch_id_ptr = NonNull::from(&mut resolved_branch_id);

                let mut select_closure = move |task| {
                    __select__(recv_slot_ptr, resolved_branch_id_ptr, general_state, task, #(#fn_select_args),*)
                };

                unsafe {
                    local_executor().invoke_call(orengine::runtime::Call::call_fn(&mut select_closure));
                    orengine::runtime::Task::park_current_task().await;
                };

                // Task is unparked here. So, we can read the result

                match resolved_branch_id {
                    #(#match_arms),*
                    _ => orengine::utils::hints::unreachable_hint()
                }
            }
        }
    };

    expanded.into()
}
