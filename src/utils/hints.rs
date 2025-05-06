//! Hints to the compiler that affects how code should be emitted or optimized.

/// Do the same as [`assert_unchecked`](std::hint::assert_unchecked), but instead of UB,
/// it panics with `debug_assertions`.
///
/// # Panics
///
/// Panics with `debug_assertions` if `cond` is `false`.
#[inline(always)]
#[allow(unused_variables, reason = "It contains #[cfg(debug_assertions)]")]
pub fn assert_hint(cond: bool, debug_msg: &str) {
    if cfg!(debug_assertions) {
        assert!(cond, "{}", debug_msg);
    } else {
        unsafe { std::hint::assert_unchecked(cond) };
    }
}

/// Do the same as [`unreachable_unchecked`](std::hint::unreachable_unchecked), but instead of UB,
/// it panics with `debug_assertions`.
///
/// # Panics
///
/// Panics with `debug_assertions`.
#[inline(always)]
#[allow(unused_variables, reason = "It contains #[cfg(debug_assertions)]")]
pub fn unreachable_hint() -> ! {
    if cfg!(debug_assertions) {
        unreachable!();
    } else {
        unsafe { std::hint::unreachable_unchecked() }
    }
}

/// Indicate that a given branch is **not** likely to be taken, relatively speaking.
#[inline(always)]
#[cold]
pub(crate) const fn cold_path() {}

/// Indicate that a given condition is likely to be true.
#[inline(always)]
pub(crate) const fn likely(b: bool) -> bool {
    if !b {
        cold_path();
    }

    b
}

/// Indicate that a given condition is likely to be false.
#[inline(always)]
pub(crate) const fn unlikely(b: bool) -> bool {
    if b {
        cold_path();
    }

    b
}