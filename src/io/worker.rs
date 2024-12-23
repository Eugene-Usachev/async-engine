use crate::io::config::IoWorkerConfig;
use crate::io::io_request_data::IoRequestData;
use crate::io::sys::{MessageRecvHeader, OpenHow, OsMessageHeader, RawFd, WorkerSys};
use crate::BUG_MESSAGE;
use nix::libc;
use nix::libc::sockaddr;
use std::cell::UnsafeCell;
use std::net::Shutdown;
use std::time::{Duration, Instant};

thread_local! {
    /// Thread-local worker for async io operations.
    pub(crate) static LOCAL_WORKER: UnsafeCell<Option<WorkerSys>> = const {
        UnsafeCell::new(None)
    };
}

/// Returns the thread-local worker wrapped in an [`Option`].
pub(crate) fn get_local_worker_ref() -> &'static mut Option<WorkerSys> {
    LOCAL_WORKER.with(|local_worker| unsafe { &mut *local_worker.get() })
}

/// Initializes the thread-local worker.
pub(crate) unsafe fn init_local_worker(config: IoWorkerConfig) {
    assert!(!get_local_worker_ref().is_some(), "{BUG_MESSAGE}");

    *get_local_worker_ref() = Some(WorkerSys::new(config));
}

/// Returns the thread-local worker.
///
/// # Panics
///
/// If the thread-local worker has not been initialized.
///
/// # Undefined Behavior
///
/// If the thread-local worker has not been initialized in `release` mode.
#[inline(always)]
pub(crate) fn local_worker() -> &'static mut WorkerSys {
    #[cfg(debug_assertions)]
    {
        get_local_worker_ref().as_mut().expect(
            "An attempt to call io-operation has failed, \
             because an Executor has no io-worker. Look at the config of the Executor.",
        )
    }

    #[cfg(not(debug_assertions))]
    unsafe {
        get_local_worker_ref().as_mut().unwrap_unchecked()
    }
}

/// A worker for async io operations.
pub(crate) trait IoWorker {
    /// Creates a new worker.
    fn new(config: IoWorkerConfig) -> Self;
    /// Registers a new time-bounded io task. It will be cancelled if the deadline is reached.
    ///
    /// It takes `&mut Instant` as a deadline because it increments the deadline by 1 nanosecond
    /// if it is not unique.
    fn register_time_bounded_io_task(
        &mut self,
        io_request_data: &IoRequestData,
        deadline: &mut Instant,
    );
    /// Deregisters a time-bounded io task.
    /// It is used to say [`IoWorker`] to not cancel the task.
    ///
    /// Deadline is always unique, therefore we can use it as a key.
    fn deregister_time_bounded_io_task(&mut self, deadline: &Instant);
    /// Returns whether `worker` has work to do.
    fn has_work(&self) -> bool;
    /// Submits an accumulated tasks to the kernel and polls it for completion if needed.
    ///
    /// It also gets `timeout` for polling. If it is `None`, it will not wait (__busy polling__).
    fn must_poll(&mut self, timeout_option: Option<Duration>);
    /// Registers a new `socket` io operation.
    fn socket(
        &mut self,
        domain: socket2::Domain,
        sock_type: socket2::Type,
        protocol: socket2::Protocol,
        request_ptr: *mut IoRequestData,
    );
    /// Registers a new `accept` io operation.
    fn accept(
        &mut self,
        listen_fd: RawFd,
        addr: *mut sockaddr,
        addrlen: *mut libc::socklen_t,
        request_ptr: *mut IoRequestData,
    );
    /// Registers a new `accept` io operation with deadline.
    fn accept_with_deadline(
        &mut self,
        listen_fd: RawFd,
        addr: *mut sockaddr,
        addrlen: *mut libc::socklen_t,
        request_ptr: *mut IoRequestData,
        deadline: &mut Instant,
    ) {
        self.register_time_bounded_io_task(unsafe { &*request_ptr } as _, deadline);
        self.accept(listen_fd, addr, addrlen, request_ptr);
    }
    /// Registers a new `connect` io operation.
    fn connect(
        &mut self,
        socket_fd: RawFd,
        addr_ptr: *const sockaddr,
        addr_len: libc::socklen_t,
        request_ptr: *mut IoRequestData,
    );
    /// Registers a new `connect` io operation with deadline.
    fn connect_with_deadline(
        &mut self,
        socket_fd: RawFd,
        addr_ptr: *const sockaddr,
        addr_len: libc::socklen_t,
        request_ptr: *mut IoRequestData,
        deadline: &mut Instant,
    ) {
        self.register_time_bounded_io_task(unsafe { &*request_ptr } as _, deadline);
        self.connect(socket_fd, addr_ptr, addr_len, request_ptr);
    }

    // region poll fd

    /// Registers a new `poll` for readable io operation.
    fn poll_fd_read(&mut self, fd: RawFd, request_ptr: *mut IoRequestData);
    /// Registers a new `poll` for readable io operation with deadline.
    fn poll_fd_read_with_deadline(
        &mut self,
        fd: RawFd,
        request_ptr: *mut IoRequestData,
        deadline: &mut Instant,
    ) {
        self.register_time_bounded_io_task(unsafe { &*request_ptr } as _, deadline);
        self.poll_fd_read(fd, request_ptr);
    }
    /// Registers a new `poll` for writable io operation.
    fn poll_fd_write(&mut self, fd: RawFd, request_ptr: *mut IoRequestData);
    /// Registers a new `poll` for writable io operation with deadline.
    fn poll_fd_write_with_deadline(
        &mut self,
        fd: RawFd,
        request_ptr: *mut IoRequestData,
        deadline: &mut Instant,
    ) {
        self.register_time_bounded_io_task(unsafe { &*request_ptr } as _, deadline);
        self.poll_fd_write(fd, request_ptr);
    }

    // endregion

    // region recv

    /// Registers a new `recv` io operation.
    fn recv(&mut self, fd: RawFd, ptr: *mut u8, len: u32, request_ptr: *mut IoRequestData);
    /// Registers a new `recv` io operation with __fixed__ buffer.
    fn recv_fixed(
        &mut self,
        fd: RawFd,
        ptr: *mut u8,
        len: u32,
        buf_index: u16,
        request_ptr: *mut IoRequestData,
    );

    /// Registers a new `recv` io operation with deadline.
    fn recv_with_deadline(
        &mut self,
        fd: RawFd,
        ptr: *mut u8,
        len: u32,
        request_ptr: *mut IoRequestData,
        deadline: &mut Instant,
    ) {
        self.register_time_bounded_io_task(unsafe { &*request_ptr } as _, deadline);
        self.recv(fd, ptr, len, request_ptr);
    }

    /// Registers a new `recv` io operation with deadline with __fixed__ buffer.
    fn recv_fixed_with_deadline(
        &mut self,
        fd: RawFd,
        ptr: *mut u8,
        len: u32,
        buf_index: u16,
        request_ptr: *mut IoRequestData,
        deadline: &mut Instant,
    ) {
        self.register_time_bounded_io_task(unsafe { &*request_ptr } as _, deadline);
        self.recv_fixed(fd, ptr, len, buf_index, request_ptr);
    }

    // endregion

    // region recv_from

    /// Registers a new `recv_from` io operation.
    // TODO with fixed buffer
    fn recv_from(
        &mut self,
        fd: RawFd,
        msg_header: &mut MessageRecvHeader,
        request_ptr: *mut IoRequestData,
    );

    /// Registers a new `recv_from` io operation with deadline.
    fn recv_from_with_deadline(
        &mut self,
        fd: RawFd,
        msg_header: &mut MessageRecvHeader,
        request_ptr: *mut IoRequestData,
        deadline: &mut Instant,
    ) {
        self.register_time_bounded_io_task(unsafe { &*request_ptr } as _, deadline);
        self.recv_from(fd, msg_header, request_ptr);
    }

    // endregion

    // region send

    /// Registers a new `send` io operation.
    fn send(&mut self, fd: RawFd, ptr: *const u8, len: u32, request_ptr: *mut IoRequestData);
    /// Registers a new `send` io operation with __fixed__ buffer.
    fn send_fixed(
        &mut self,
        fd: RawFd,
        ptr: *const u8,
        len: u32,
        buf_index: u16,
        request_ptr: *mut IoRequestData,
    );

    /// Registers a new `send` io operation with deadline.
    fn send_with_deadline(
        &mut self,
        fd: RawFd,
        ptr: *const u8,
        len: u32,
        request_ptr: *mut IoRequestData,
        deadline: &mut Instant,
    ) {
        self.register_time_bounded_io_task(unsafe { &*request_ptr } as _, deadline);
        self.send(fd, ptr, len, request_ptr);
    }

    /// Registers a new `send` io operation with deadline with __fixed__ buffer.
    fn send_fixed_with_deadline(
        &mut self,
        fd: RawFd,
        ptr: *const u8,
        len: u32,
        buf_index: u16,
        request_ptr: *mut IoRequestData,
        deadline: &mut Instant,
    ) {
        self.register_time_bounded_io_task(unsafe { &*request_ptr } as _, deadline);
        self.send_fixed(fd, ptr, len, buf_index, request_ptr);
    }

    // endregion

    // region send_to

    /// Registers a new `send_to` io operation.
    // TODO with fixed buffer
    fn send_to(
        &mut self,
        fd: RawFd,
        msg_header: *const OsMessageHeader,
        request_ptr: *mut IoRequestData,
    );
    /// Registers a new `send_to` io operation with deadline.
    fn send_to_with_deadline(
        &mut self,
        fd: RawFd,
        msg_header: *const OsMessageHeader,
        request_ptr: *mut IoRequestData,
        deadline: &mut Instant,
    ) {
        self.register_time_bounded_io_task(unsafe { &*request_ptr } as _, deadline);
        self.send_to(fd, msg_header, request_ptr);
    }

    // endregion

    // region peek

    /// Registers a new `peek` io operation.
    fn peek(&mut self, fd: RawFd, ptr: *mut u8, len: u32, request_ptr: *mut IoRequestData);
    /// Registers a new `peek` io operation with __fixed__ buffer.
    fn peek_fixed(
        &mut self,
        fd: RawFd,
        ptr: *mut u8,
        len: u32,
        buf_index: u16,
        request_ptr: *mut IoRequestData,
    );

    /// Registers a new `peek` io operation with deadline.
    fn peek_with_deadline(
        &mut self,
        fd: RawFd,
        ptr: *mut u8,
        len: u32,
        request_ptr: *mut IoRequestData,
        deadline: &mut Instant,
    ) {
        self.register_time_bounded_io_task(unsafe { &*request_ptr } as _, deadline);
        self.peek(fd, ptr, len, request_ptr);
    }

    /// Registers a new `peek` io operation with deadline with __fixed__ buffer.
    fn peek_fixed_with_deadline(
        &mut self,
        fd: RawFd,
        ptr: *mut u8,
        len: u32,
        buf_index: u16,
        request_ptr: *mut IoRequestData,
        deadline: &mut Instant,
    ) {
        self.register_time_bounded_io_task(unsafe { &*request_ptr } as _, deadline);
        self.peek_fixed(fd, ptr, len, buf_index, request_ptr);
    }

    // endregion

    // region peek_from

    /// Registers a new `peek_from` io operation.
    // TODO with fixed buffer
    fn peek_from(
        &mut self,
        fd: RawFd,
        msg: &mut MessageRecvHeader,
        request_ptr: *mut IoRequestData,
    );
    /// Registers a new `peek_from` io operation with deadline.
    fn peek_from_with_deadline(
        &mut self,
        fd: RawFd,
        msg: &mut MessageRecvHeader,
        request_ptr: *mut IoRequestData,
        deadline: &mut Instant,
    ) {
        self.register_time_bounded_io_task(unsafe { &*request_ptr } as _, deadline);
        self.peek_from(fd, msg, request_ptr);
    }

    // endregion

    /// Registers a new `shutdown` io operation.
    fn shutdown(&mut self, fd: RawFd, how: Shutdown, request_ptr: *mut IoRequestData);
    /// Registers a new `open` io operation.
    fn open(
        &mut self,
        path: *const libc::c_char,
        open_how: *const OpenHow,
        request_ptr: *mut IoRequestData,
    );
    /// Registers a new `fallocate` io operation if the kernel supports it.
    fn fallocate(
        &mut self,
        fd: RawFd,
        offset: u64,
        len: u64,
        flags: i32,
        request_ptr: *mut IoRequestData,
    );
    /// Registers a new `sync_all` io operation.
    fn sync_all(&mut self, fd: RawFd, request_ptr: *mut IoRequestData);
    /// Registers a new `sync_data` io operation.
    fn sync_data(&mut self, fd: RawFd, request_ptr: *mut IoRequestData);

    // region read

    /// Registers a new `read` io operation.
    fn read(&mut self, fd: RawFd, ptr: *mut u8, len: u32, request_ptr: *mut IoRequestData);
    /// Registers a new `read` io operation with __fixed__ buffer.
    fn read_fixed(
        &mut self,
        fd: RawFd,
        ptr: *mut u8,
        len: u32,
        buf_index: u16,
        request_ptr: *mut IoRequestData,
    );
    /// Registers a new `pread` io operation.
    fn pread(
        &mut self,
        fd: RawFd,
        ptr: *mut u8,
        len: u32,
        offset: usize,
        request_ptr: *mut IoRequestData,
    );
    /// Registers a new `pread` io operation with __fixed__ buffer.
    fn pread_fixed(
        &mut self,
        fd: RawFd,
        ptr: *mut u8,
        len: u32,
        buf_index: u16,
        offset: usize,
        request_ptr: *mut IoRequestData,
    );

    // endregion

    // region write

    /// Registers a new `write` io operation.
    fn write(&mut self, fd: RawFd, ptr: *const u8, len: u32, request_ptr: *mut IoRequestData);
    /// Registers a new `write` io operation with __fixed__ buffer.
    fn write_fixed(
        &mut self,
        fd: RawFd,
        ptr: *const u8,
        len: u32,
        buf_index: u16,
        request_ptr: *mut IoRequestData,
    );
    /// Registers a new `pwrite` io operation.
    fn pwrite(
        &mut self,
        fd: RawFd,
        ptr: *const u8,
        len: u32,
        offset: usize,
        request_ptr: *mut IoRequestData,
    );
    /// Registers a new `pwrite` io operation with __fixed__ buffer.
    fn pwrite_fixed(
        &mut self,
        fd: RawFd,
        ptr: *const u8,
        len: u32,
        buf_index: u16,
        offset: usize,
        request_ptr: *mut IoRequestData,
    );

    // endregion

    /// Registers a new `close` io operation.
    fn close(&mut self, fd: RawFd, request_ptr: *mut IoRequestData);
    /// Registers a new `rename` io operation.
    fn rename(
        &mut self,
        old_path: *const libc::c_char,
        new_path: *const libc::c_char,
        request_ptr: *mut IoRequestData,
    );
    /// Registers a new `mkdir` io operation.
    fn create_dir(&mut self, path: *const libc::c_char, mode: u32, request_ptr: *mut IoRequestData);
    /// Registers a new `unlink` io operation.
    fn remove_file(&mut self, path: *const libc::c_char, request_ptr: *mut IoRequestData);
    /// Registers a new `rmdir` io operation.
    fn remove_dir(&mut self, path: *const libc::c_char, request_ptr: *mut IoRequestData);
}
