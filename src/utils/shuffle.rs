use std::cell::Cell;
use std::num::Wrapping;

/// Randomly shuffles a slice.
// Copied from https://github.com/crossbeam-rs/crossbeam/blob/master/crossbeam-channel/src/utils.rs
pub fn shuffle<T>(v: &mut [T]) {
    let len = v.len();
    if len <= 1 {
        return;
    }

    std::thread_local! {
        static RNG: Cell<Wrapping<u32>> = const { Cell::new(Wrapping(1_406_868_647)) };
    }

    let _ = RNG.try_with(|rng| {
        for i in 1..len {
            let mut x = rng.get();
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            rng.set(x);

            let x = x.0;
            let n = i + 1;
            let j = (u64::from(x).wrapping_mul(n as u64) >> 32) as u32 as usize;

            v.swap(i, j);
        }
    });
}
