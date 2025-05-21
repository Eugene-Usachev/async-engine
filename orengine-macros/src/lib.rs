//! This crate provides tools to generate code with using `orengine` crate.
#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![deny(clippy::blanket_clippy_restriction_lints)]
#![warn(clippy::nursery)]
#![allow(clippy::implicit_return)]
#![allow(clippy::indexing_slicing)]
#![allow(clippy::panic)]

extern crate proc_macro;
mod ident_helper;
mod select;

use proc_macro::TokenStream;
use quote::quote;
use std::time::Duration;
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

/// Generates a test function with a provided locality.
fn generate_test(input: TokenStream, is_local: bool, timeout: Option<Duration>) -> TokenStream {
    // TODO timeout
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
            #spawn_fn(|| async {
                #body
            }, None); // TODO timeout
            println!("Test {} finished!", #name_str.to_string());
        }
    };

    TokenStream::from(expanded)
}

/// Generates a test function by running an `Executor` with a `local` task.
///
/// # The difference between `test_local` and [`test_shared()`]
///
/// `test_local` generates a test function that runs an `Executor` with a `local` task.
/// [`test_shared()`] generates a test function that runs an
/// `Executor` with a `shared` task.
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
/// The code above is equal to:
///
/// ```ignore
/// #[test]
/// fn test_sleep() {
///     println!("Test sleep started!");
///
///     orengine::test::run_test_and_block_on_local(async {
///         let start = std::time::Instant::now();
///
///         orengine::sleep(std::time::Duration::from_secs(1)).await;
///
///         assert!(start.elapsed() >= std::time::Duration::from_secs(1));
///     });
///
///     println!("Test sleep finished!");
/// }
/// ```
#[proc_macro_attribute]
pub fn test_local(_: TokenStream, input: TokenStream) -> TokenStream {
    generate_test(input, true, None) // TODO timeout
}

/// Generates a test function by running an `Executor` with a `local` task.
///
/// # The difference between `test_shared` and [`test_local()`]
///
/// [`test_shared()`] generates a test function that runs an `Executor` with a `shared` task.
/// `test_local` generates a test function that runs an `Executor` with a `local` task.
///
/// # Example
///
/// ```ignore
/// #[orengine::test::test_shared]
/// fn test_sleep() {
///     let start = std::time::Instant::now();
///
///     orengine::sleep(std::time::Duration::from_secs(1)).await;
///
///     assert!(start.elapsed() >= std::time::Duration::from_secs(1));
/// }
/// ```
///
/// # Note
///
/// The code above is equal to:
///
/// ```ignore
/// #[test]
/// fn test_sleep() {
///     println!("Test sleep started!");
///
///     orengine::test::run_test_and_block_on_shared(async {
///         let start = std::time::Instant::now();
///
///         orengine::sleep(std::time::Duration::from_secs(1)).await;
///
///         assert!(start.elapsed() >= std::time::Duration::from_secs(1));
///     });
///
///     println!("Test sleep finished!");
/// }
/// ```
#[proc_macro_attribute]
pub fn test_shared(_: TokenStream, input: TokenStream) -> TokenStream {
    generate_test(input, false, None) // TODO timeout
}

///# `select!` Macro
///
/// The `select!` macro provides a way to wait on multiple asynchronous channel operations
/// simultaneously, executing the code corresponding to the first operation that becomes ready.
/// It is similar in concept to the Go's `select` statement.
///
/// This macro can be used for both blocking and non-blocking selections.
///
/// # Syntax
///
/// The basic structure of the `select!` macro is as follows:
///
///```text
/// use orengine::select;
///
/// select! {
///     // Pattern 1: Receiving from a channel
///     recv(CHANNEL_EXPRESSION) -> PATTERN => EXPRESSION,
///
///     // Pattern 2: Sending to a channel
///     send(CHANNEL_EXPRESSION, SEND_EXPRESSION) -> PATTERN => EXPRESSION,
///
///     // Pattern 3: Default case (optional, makes the select non-blocking)
///     default => EXPRESSION
/// }
/// ```
///
/// # Behavior
///
/// ## Blocking vs. Non-Blocking
///
/// - **Blocking Select**: If a default arm is not provided, the `select!` macro will block
///   until one of the `recv` or `send` operations can complete.
///   It can be called only in `async` blocks.
/// - **Non-Blocking Select**: If a default arm is provided, the `select!` macro will first check
///   if any of the `recv` or `send` operations can be complete immediately. If none are ready,
///   the default arm's expression is executed.
///   The `select!` macro will not block.
///   Therefore, it can be called in `async` and non `async` blocks.
///
/// # Shuffling of Arms
///
/// To ensure fairness and prevent starvation if multiple arms are ready simultaneously,
/// the `select!` macro shuffles the order of all `recv` and `send` arms internally before
/// checking their readiness. The default arm, if present, is always considered last
/// and is not part of the shuffling.
///
/// This means that if, for example, both `recv(&ch1)` and `recv(&ch2)` are ready,
/// the macro doesn't deterministically pick the one listed first in the source code.
///
/// # Examples
///
/// ## 1. Non-Blocking Select with default
///
/// ```text
/// use orengine::{local_executor, select};
/// use orengine::sync::{LocalChannel, AsyncChannel, AsyncReceiver, AsyncSender};
/// use std::time::Duration;
///
/// fn non_blocking_example() {
///     let ch1 = LocalChannel::<u32>::bounded(1);
///     let ch2 = LocalChannel::<u32>::bounded(1);
///
///     // Send a message to ch2 so it's ready for recv
///     ch2.try_send(31).expect("failed to send to ch2");
///
///     let a = select! {
///         // This arm is not ready as ch1 is empty
///         recv(&ch1) -> var => {
///             println!("Received from ch1");
///             var.expect("ch1 recv failed").unwrap_or_default()
///         },
///         // This arm is ready
///         recv(&ch2) -> var => {
///             println!("Received from ch2");
///             var.expect("ch2 recv failed").unwrap_or_default()
///         },
///         // This arm is ready as ch2 is full
///         send(&ch2, 20) -> _var => {
///             println!("Sent 20 to ch2");
///             1 // Arbitrary result for this arm
///         },
///         // Default arm, executed if no other arm is immediately ready
///         default => {
///             println!("Default arm executed");
///             4 // Arbitrary result for default
///         }
///     };
///
///     assert_eq!(a, 31, "non-blocking recv assertion failed (expected value from ch2)");
///     println!("Non-blocking select result: {}", a);
///
///     // Example showing default being chosen
///     let ch3 = LocalChannel::<u32>::bounded(1); // Empty channel
///     let ch4 = LocalChannel::<u32>::bounded(0); // Empty channel, send would block if capacity is 0
///
///     let b = select! {
///         // Failed due to ch3 is empty
///         recv(&ch3) -> _ => 100,
///         // Failed due to ch4 is full
///         send(&ch4, 50) -> _ => 200,
///         default => {
///             println!("Default arm chosen for ch3/ch4");
///             42
///         }
///     };
///     assert_eq!(b, 42, "default arm was not chosen when it should have been");
///     println!("Non-blocking select (default chosen) result: {}", b);
/// }
/// ```
///
/// ## 2. Blocking Select
///
/// ```text
/// use orengine::{local_executor, select};
/// use orengine::sync::{LocalChannel, AsyncChannel, AsyncReceiver, AsyncSender};
/// use std::time::Duration;
/// use std::rc::Rc;
///
/// async fn blocking_example() {
///     let ch1 = LocalChannel::<u32>::bounded(1); // Empty, recv would block
///     let ch2 = Rc::new(LocalChannel::<u32>::bounded(1)); // Will receive a message
///     let ch2_clone = ch2.clone();
///     let ch3 = LocalChannel::<u32>::bounded(0); // Send would block as capacity is 0 and no receiver
///
///     // Spawn a task to send a message to ch2 after a short delay
///     local_executor().spawn_local(async move {
///         orengine::sleep(Duration::from_micros(100)).await; // Example with tokio sleep
///
///         ch2_clone.send(31).await.expect("failed to send to ch2_clone");
///
///         println!("Message sent to ch2_clone");
///     });
///
///     println!("Blocking select will now wait...");
///
///     let a = select! {
///         // This arm would block as ch1 is empty
///         recv(&ch1) -> var => {
///             println!("Received from ch1 (blocking)");
///             var.expect("ch1 recv failed (blocking)").unwrap_or_default()
///         },
///         // This arm will eventually be ready after the spawned task sends a message
///         recv(&ch2) -> var => {
///             println!("Received from ch2 (blocking)");
///             var.expect("ch2 recv failed (blocking)").unwrap_or_default()
///         },
///         // This arm would block as ch3 has 0 capacity and no active receiver
///         send(&ch3, 20) -> _var => {
///             println!("Sent 20 to ch3 (blocking)");
///             1 // Arbitrary result for this arm
///         }
///         // No default arm, so this select is blocking
///     };
///
///     assert_eq!(a, 31, "blocking recv assertion failed");
///     println!("Blocking select result: {}", a);
/// }
/// ```
///
/// # One case and default optimizations
///
/// The `select!` macro includes an optimization for a common pattern:
/// a single `recv` or `send` arm combined with a default arm.
/// - If you have `select! { recv(&ch) -> var => ..., default => ... }`,
///   it is optimized to effectively become a `ch.try_recv()`.
/// - If you have `select! { send(&ch, val) -> res => ..., default => ... }`,
///   it is optimized to effectively become a `ch.try_send(val)`.
#[proc_macro]
pub fn select(input: TokenStream) -> TokenStream {
    select::select(input, false)
}
