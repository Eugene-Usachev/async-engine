//! This module provides a comprehensive set of asynchronous traits and utilities for working
//! with synchronization primitives.
//!
//! # Channels
//!
//! ## Traits
//!
//! - [`AsyncSender`]
//! - [`AsyncReceiver`]
//! - [`AsyncChannel`]
//!
//! ## Implementations
//!
//! In `local` context: [`LocalChannel`]
//!
//! In `global` context: [`Channel`]
//!
//! # Condition Variables
//!
//! ## Traits
//!
//! - [`AsyncCondVar`]
//!
//! ## Implementations
//!
//! In `local` context: [`LocalCondVar`]
//!
//! In `global` context: [`CondVar`]
//!
//! # Mutexes
//!
//! ## Traits
//!
//! - [`AsyncMutex`]
//! - [`AsyncMutexGuard`]
//! - [`AsyncSubscribableMutex`]
//!
//! ## Implementations
//!
//! In `local` context: [`LocalMutex`]
//!
//! In `global` context: [`Mutex`], [`NaiveMutex`]
//!
//! # Once
//!
//! ## Traits
//!
//! - [`AsyncOnce`]
//!
//! ## Implementations
//!
//! In `local` context: [`LocalOnce`]
//!
//! In `global` context: [`Once`]
//!
//! # RW Locks
//!
//! ## Traits
//!
//! - [`AsyncRWLock`]
//! - [`AsyncReadLockGuard`]
//! - [`AsyncWriteLockGuard`]
//!
//! ## Implementations
//!
//! In `local` context: [`LocalRWLock`]
//!
//! In `global` context: [`RWLock`]
//!
//! # Wait Groups
//!
//! ## Traits
//!
//! - [`AsyncWaitGroup`]
//!
//! ## Implementations
//!
//! In `local` context: [`LocalWaitGroup`]
//!
//! In `global` context: [`WaitGroup`]

pub use channels::*;
pub use cond_vars::*;
pub use mutexes::*;
pub use onces::*;
pub use rw_locks::*;
pub use wait_groups::*;

pub mod channels;
pub mod cond_vars;
pub mod mutexes;
pub mod onces;
pub mod rw_locks;
pub mod wait_groups;
