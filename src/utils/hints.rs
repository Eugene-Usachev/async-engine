/// Do the same as [`assert_unchecked`](std::hint::assert_unchecked), but instead of UB,
/// it panics with `debug_assertions`.
#[inline(always)]
#[allow(unused_variables, reason = "It contains #[cfg(debug_assertions)]")]
pub fn assert_hint(cond: bool, debug_msg: &str) {
    #[cfg(debug_assertions)]
    {
        assert!(cond, "{}", debug_msg);
    }

    #[cfg(not(debug_assertions))]
    unsafe {
        std::hint::assert_unchecked(cond)
    };
}

/// Do the same as [`unreachable_unchecked`](std::hint::unreachable_unchecked), but instead of UB,
/// it panics with `debug_assertions`.
#[inline(always)]
#[allow(unused_variables, reason = "It contains #[cfg(debug_assertions)]")]
pub fn unreachable_hint() -> ! {
    #[cfg(debug_assertions)]
    unreachable!();

    #[cfg(not(debug_assertions))]
    unsafe {
        std::hint::unreachable_unchecked()
    }
}
