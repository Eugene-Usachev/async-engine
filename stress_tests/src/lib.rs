#[cfg(test)]
mod local_channel;
#[cfg(test)]
mod mutex;
#[cfg(test)]
mod select;
#[cfg(test)]
mod shared_channel;
#[cfg(test)]
mod spin_lock;

#[allow(
    dead_code,
    reason = "It is available only in tests and it is run only in tests."
)]
static GLOBAL_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[allow(
    dead_code,
    reason = "It is available only in tests and it is run only in tests."
)]
struct UnsafeSend<T>(T);

#[allow(
    clippy::non_send_fields_in_send_ty,
    reason = "It is guaranteed by the caller."
)]
unsafe impl<T> Send for UnsafeSend<T> {}

#[allow(
    dead_code,
    reason = "It is available only in tests and it is run only in tests."
)]
pub(crate) fn acquire_global_lock() -> UnsafeSend<std::sync::MutexGuard<'static, ()>> {
    UnsafeSend(GLOBAL_LOCK.lock().unwrap())
}
