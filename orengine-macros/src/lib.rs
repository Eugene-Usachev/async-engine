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
mod test;

use crate::test::{generate_test, parse_test_args};
use proc_macro::TokenStream;

/// Generates a test function by running an `Executor` with a `local` task.
///
/// # The difference between `test_local` and [`test_shared()`]
///
/// `test_local` generates a test function that runs an `Executor` with a `local` task.
/// [`test_shared()`] generates a test function that runs an
/// `Executor` with a `shared` task.
///
/// You also can pass timeout to the test function by the argument `timeout_ms`.
///
/// # Examples
///
/// ## Without timeout
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
/// ## With timeout
///
/// ```ignore
/// #[orengine::test::test_local(timeout_ms = 100)]
/// #[should_panic = "Test timed out"]
/// fn test_sleep() {
///     orengine::sleep(std::time::Duration::from_secs(3)).await;
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
///     orengine::test::run_test_and_block_on_local(async {
///         println!("Test sleep started!");
///
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
pub fn test_local(args: TokenStream, input: TokenStream) -> TokenStream {
    let (timeout, exclusive_in) = parse_test_args(args);

    generate_test(input, true, timeout, exclusive_in)
}

// TODO exclusive
/// Generates a test function by running an `Executor` with a `local` task.
///
/// # The difference between `test_shared` and [`test_local()`]
///
/// [`test_shared()`] generates a test function that runs an `Executor` with a `shared` task.
/// `test_local` generates a test function that runs an `Executor` with a `local` task.
///
/// You also can pass timeout to the test function by the argument `timeout_ms`.
///
/// # Examples
///
/// ## Without timeout
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
/// ## With timeout
///
/// ```ignore
/// #[orengine::test::test_shared(timeout_ms = 100)]
/// #[should_panic = "Test timed out"]
/// fn test_sleep() {
///     orengine::sleep(std::time::Duration::from_secs(3)).await;
/// }
/// ```
///
///
/// # Note
///
/// The code above is equal to:
///
/// ```ignore
/// #[test]
/// fn test_sleep() {
///     orengine::test::run_test_and_block_on_shared(async {
///         println!("Test sleep started!");
///
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
pub fn test_shared(args: TokenStream, input: TokenStream) -> TokenStream {
    let (timeout, exclusive_in) = parse_test_args(args);

    generate_test(input, false, timeout, exclusive_in)
}

///# `select!` Macro
///
/// The `select!` macro provides a way to wait on multiple asynchronous channel operations
/// simultaneously, executing the code corresponding to the first operation that becomes ready.
/// It is similar in concept to the Go's `select` statement.
///
/// This macro can be used for both blocking and non-blocking selections.
///
/// It also supports `deadline` and `timeout` patterns.
/// Read examples below for more information.
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
///     // Pattern 3: Default case (optional (can be at most one optional case), makes the select non-blocking)
///     default => EXPRESSION
///
///     // Pattern 4: Timeout case (optional (can be at most one optional case), sets a deadline for the select)
///     deadline(Instant) => EXPRESSION
///
///     // Pattern 5: Timeout case (optional (can be at most one optional case), sets a deadline for the select)
///     timeout(DURATION) => EXPRESSION
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
///   if any of the `recv` or `send` operations can be complete immediately. If none is ready,
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
///         orengine::sleep(Duration::from_micros(100)).await; // Example with sleep
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
/// ## 3. Blocking Select with timeout
///
/// ```text
/// use orengine::{local_executor, select};
/// use orengine::sync::{LocalChannel, AsyncChannel, AsyncReceiver, AsyncSender};
/// use std::time::{Instant, Duration};
///
/// async fn blocking_select_with_timeout() {
///     let ch1 = LocalChannel::<u32>::bounded(1); // Empty, recv would block
///     let ch2 = LocalChannel::<u32>::bounded(1); // Empty, recv would block
///
///     println!("Blocking select with timeout will now wait...");
///
///     let a = select! {
///         // This arm would block as ch1 is empty
///         recv(&ch1) -> var => {
///             println!("Received from ch1 (blocking)");
///             var.expect("ch1 recv failed (blocking)").unwrap_or_default()
///         },
///         // This arm would block as ch2 is empty
///         recv(&ch2) -> var => {
///             println!("Received from ch2 (blocking)");
///             var.expect("ch2 recv failed (blocking)").unwrap_or_default()
///         },
///         // This arm will be ready after a short delay
///         timeout(Duration::from_micros(100)) => 31,
///
///         // Or we could use the `deadline`
///         // deadline(Instant::now() + Duration::from_micros(100)) => 31,
///     };
///
///     assert_eq!(a, 31, "blocking recv assertion failed");
///     println!("Blocking select with timeout result: {}", a);
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
