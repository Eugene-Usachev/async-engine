//! This module contains [`poll_for_io_request`] and [`poll_for_time_bounded_io_request`] macros.

mod macros {
    /// This macro creates a code block that sends the IO request to the worker and
    /// waits for the result.
    ///
    /// It should be used with [`Future`].
    ///
    /// # Arguments
    ///
    /// * `$do_request:block` - code block that sends the IO request to the worker.
    /// * `$io_request_data:expr` - expression that contains the `&mut Option(`[`IoRequestData`]`)`.
    /// * `$cx:expr` - expression that contains the [`Context`](std::task::Context).
    /// * `$result_ident:ident` - identifier that will be used to store the result.
    /// * `$return_expr:expr` - expression that will be returned when the result is ready.
    ///
    /// [`IO request data`]: crate::io::io_request_data::IoRequestData
    macro_rules! poll_for_io_request {
        ($do_request:block, $io_request_data:expr, $cx:expr, $result_ident:ident, $return_expr:expr) => {
            if $io_request_data.is_some() {
                // We can just read it, without taking, because it results in the same, but it has more performance.
                let mut io_request_data = unsafe {
                    std::ptr::read($crate::utils::unwrap_or_bug_hint($io_request_data.as_ref()))
                };

                match io_request_data.ret() {
                    Ok($result_ident) => {
                        return Poll::Ready(Ok($return_expr));
                    }
                    Err(err) => {
                        return Poll::Ready(Err(err));
                    }
                }
            }

            let task = unsafe { $crate::runtime::Task::from_context($cx) };
            $io_request_data = Some(IoRequestData::new(task));

            $do_request;

            return Poll::Pending;
        };
    }

    /// This macro creates a code block that sends the time-bounded IO request to the worker and
    /// waits for the result.
    ///
    /// It should be used with [`Future`].
    ///
    /// # Arguments
    ///
    /// * `$do_request:block` - code block that sends the IO request to the worker.
    /// * `$io_request_data:expr` - expression that contains the `&mut Option(`[`IoRequestData`]`)`.
    /// * `$worker:expr` - expression that contains the [`IoWorker`](crate::io::worker::IoWorker).
    /// * `$deadline:expr` - expression that contains the `&`[`OrengineInstant`].
    /// * `$cx:expr` - expression that contains the [`Context`](std::task::Context).
    /// * `$result_ident:ident` - identifier that will be used to store the result.
    /// * `$return_expr:expr` - expression that will be returned when the result is ready.
    ///
    /// [`IO request data`]: crate::io::io_request_data::IoRequestData
    /// [`OrengineInstant`]: crate::utils::OrengineInstant
    macro_rules! poll_for_time_bounded_io_request {
        (
            $do_request:block,
            $io_request_data:expr,
            $worker:expr,
            $deadline:expr,
            $cx:expr,
            $result_ident:ident,
            $return_expr:expr
        ) => {
            if $io_request_data.is_some() {
                // We can just read it, without taking, because it results in the same, but it has more performance.
                let mut io_request_data = unsafe {
                    std::ptr::read($crate::utils::unwrap_or_bug_hint($io_request_data.as_ref()))
                };

                match io_request_data.ret() {
                    Ok($result_ident) => {
                        $worker.deregister_time_bounded_io_task($deadline);

                        return Poll::Ready(Ok($return_expr));
                    }
                    Err(err) => {
                        if err.kind() != std::io::ErrorKind::TimedOut {
                            $worker.deregister_time_bounded_io_task($deadline);
                        }

                        return Poll::Ready(Err(err));
                    }
                }
            }

            let task = unsafe { $crate::runtime::Task::from_context($cx) };
            $io_request_data = Some(IoRequestData::new(task));

            $do_request;

            return Poll::Pending;
        };
    }

    pub(crate) use {poll_for_io_request, poll_for_time_bounded_io_request};
}

pub(crate) use macros::{poll_for_io_request, poll_for_time_bounded_io_request};
