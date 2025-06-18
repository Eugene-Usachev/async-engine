//! This module contains definitions for the Linux buffer:
//! [`FixedBuffer`](linux_buffer::FixedBuffer) and [`LinuxBuffer`](linux_buffer::LinuxBuffer).

#[cfg(target_os = "linux")]
pub mod linux_buffer;
