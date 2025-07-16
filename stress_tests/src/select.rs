use crate::sync_wait_wg;
// region `local` stress tests
use orengine::sync::{
    AsyncChannel, AsyncSender, AsyncWaitGroup, Channel, LocalChannel, LocalWaitGroup, WaitGroup,
};
use orengine::test::sched_future;
use orengine::{local_executor, select, yield_now, Local};
use std::rc::Rc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::{Relaxed, Release};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[allow(clippy::future_not_send, reason = "It is local")]
async fn test_local_select_stress(with_default: bool) {
    #[cfg(debug_assertions)]
    const TRIES: usize = 3;
    #[cfg(not(debug_assertions))]
    const TRIES: usize = 10;
    const FIRST_SHIFT: usize = 12;
    const SECOND_SHIFT: usize = 21;
    const THIRD_SHIFT: usize = 30;
    const PAR_MULTIPLIER: usize = 3;
    const COUNT: usize = 1000;

    type ChanCreator = fn() -> LocalChannel<usize>;

    fn full_bounded_chan_creator() -> LocalChannel<usize> {
        LocalChannel::bounded(COUNT)
    }

    fn one_bounded_chan_creator() -> LocalChannel<usize> {
        LocalChannel::bounded(1)
    }

    fn zero_bounded_chan_creator() -> LocalChannel<usize> {
        LocalChannel::bounded(0)
    }

    #[allow(clippy::future_not_send, reason = "It is local")]
    async fn stress_test_with_creators_select(
        with_default: bool,
        chan_1_creator: ChanCreator,
        chan_2_creator: ChanCreator,
        chan_3_creator: ChanCreator,
        chan_4_creator: ChanCreator,
    ) {
        let chan1 = Rc::new(chan_1_creator());
        let chan2 = Rc::new(chan_2_creator());
        let chan3 = Rc::new(chan_3_creator());
        let chan4 = Rc::new(chan_4_creator());
        let total_sent = Local::new(0);
        let total_received = Local::new(0);
        let wg = Rc::new(LocalWaitGroup::new());

        for _ in 0..PAR_MULTIPLIER {
            {
                let wg_clone = wg.clone();
                let total_received_clone = total_received.clone();
                let chan1 = chan1.clone();
                let chan2 = chan2.clone();
                let chan3 = chan3.clone();
                let chan4 = chan4.clone();

                wg.inc().await;

                local_executor().spawn_local(async move {
                    let count = if with_default { COUNT } else { COUNT * 2 };

                    for i in 0..count {
                        let wg_clone2 = wg_clone.clone();
                        let total_received_clone2 = total_received_clone.clone();
                        let chan1_clone = chan1.clone();
                        let chan2_clone = chan2.clone();
                        let chan3_clone = chan3.clone();
                        let chan4_clone = chan4.clone();

                        wg_clone2.inc().await;

                        local_executor().spawn_local(async move {
                            let received = if with_default {
                                let mut received;
                                loop {
                                    received = if i % 2 == 0 {
                                        select! {
                                            recv(&chan1_clone) -> received => Some(received)
                                            recv(&chan2_clone) -> received => Some(received)
                                            recv(&chan3_clone) -> received => Some(received)
                                            recv(&chan4_clone) -> received => Some(received)
                                            default => None
                                        }
                                    } else {
                                        select! {
                                            recv(&chan4_clone) -> received => Some(received)
                                            recv(&chan3_clone) -> received => Some(received)
                                            recv(&chan2_clone) -> received => Some(received)
                                            recv(&chan1_clone) -> received => Some(received)
                                            default => None
                                        }
                                    };

                                    if received.is_some() {
                                        break;
                                    }

                                    yield_now().await;
                                }

                                received.unwrap()
                            } else if i % 2 == 0 {
                                select! {
                                    recv(&chan1_clone) -> received => received
                                    recv(&chan2_clone) -> received => received
                                    recv(&chan3_clone) -> received => received
                                    recv(&chan4_clone) -> received => received
                                }
                            } else {
                                select! {
                                    recv(&chan4_clone) -> received => received
                                    recv(&chan3_clone) -> received => received
                                    recv(&chan2_clone) -> received => received
                                    recv(&chan1_clone) -> received => received
                                }
                            };

                            *total_received_clone2.borrow_mut() +=
                                received.expect("failed to recv");

                            wg_clone2.done().await;
                        });
                    }

                    wg_clone.done().await;
                });
            }

            {
                let wg_clone = wg.clone();
                let total_sent_clone = total_sent.clone();
                let chan1_clone = chan1.clone();
                let chan2_clone = chan2.clone();

                wg_clone.inc().await;

                local_executor().spawn_local(async move {
                    for i in 0..COUNT {
                        let wg_clone2 = wg_clone.clone();
                        let chan1_clone2 = chan1_clone.clone();
                        let chan2_clone2 = chan2_clone.clone();
                        let total_sent_clone2 = total_sent_clone.clone();

                        wg_clone2.inc().await;

                        local_executor().spawn_local(async move {
                            if i % 2 == 0 {
                                chan1_clone2.send(i).await.expect("failed to send");

                                *total_sent_clone2.borrow_mut() += i;
                            } else {
                                chan2_clone2
                                    .send(i << FIRST_SHIFT)
                                    .await
                                    .expect("failed to send");

                                *total_sent_clone2.borrow_mut() += i << FIRST_SHIFT;
                            }

                            wg_clone2.done().await;
                        });
                    }

                    wg_clone.done().await;
                });
            }

            if !with_default {
                let wg_clone = wg.clone();
                let total_sent_clone = total_sent.clone();
                let chan3_clone = chan3.clone();
                let chan4_clone = chan4.clone();

                wg_clone.inc().await;

                local_executor().spawn_local(async move {
                    for i in 0..COUNT {
                        let wg_clone2 = wg_clone.clone();
                        let chan3_clone2 = chan3_clone.clone();
                        let chan4_clone2 = chan4_clone.clone();
                        let total_sent_clone2 = total_sent_clone.clone();

                        wg_clone2.inc().await;

                        local_executor().spawn_local(async move {
                            let mut number_of_tries = 0;

                            loop {
                                if number_of_tries > COUNT * 30 {
                                    let stat3 = chan3_clone2.fullness_state();
                                    let stat4 = chan4_clone2.fullness_state();

                                    panic!(
                                        "Maybe try_send can't work with select. Too many tries. Stat1: {stat3:?} Stat2: {stat4:?}"
                                    );
                                };

                                let (res, n) = if i % 2 == 0 {
                                    let value = i << SECOND_SHIFT;

                                    (chan3_clone2.try_send(value), value)
                                } else {
                                    let value = i << THIRD_SHIFT;

                                    (chan4_clone2.try_send(value), value)
                                };

                                match res {
                                    Ok(()) => {
                                        *total_sent_clone2.borrow_mut() += n;

                                        break;
                                    }
                                    Err(_) => yield_now().await,
                                }

                                number_of_tries += 1;
                            }

                            wg_clone2.done().await;
                        });
                    }

                    wg_clone.done().await;
                });
            }
        }

        assert_eq!(*total_sent.borrow(), *total_received.borrow());
    }

    for _ in 0..TRIES {
        stress_test_with_creators_select(
            with_default,
            full_bounded_chan_creator,
            full_bounded_chan_creator,
            full_bounded_chan_creator,
            full_bounded_chan_creator,
        )
        .await;

        stress_test_with_creators_select(
            with_default,
            zero_bounded_chan_creator,
            zero_bounded_chan_creator,
            zero_bounded_chan_creator,
            zero_bounded_chan_creator,
        )
        .await;

        stress_test_with_creators_select(
            with_default,
            one_bounded_chan_creator,
            one_bounded_chan_creator,
            one_bounded_chan_creator,
            one_bounded_chan_creator,
        )
        .await;

        stress_test_with_creators_select(
            with_default,
            one_bounded_chan_creator,
            zero_bounded_chan_creator,
            one_bounded_chan_creator,
            zero_bounded_chan_creator,
        )
        .await;
    }
}

#[orengine::test::test_local(timeout_ms = 10000, exclusive_in = "*")]
fn test_local_select_stress_with_default() {
    test_local_select_stress(true).await;
}

#[orengine::test::test_local(timeout_ms = 10000, exclusive_in = "*")]
fn test_local_select_stress_without_default() {
    test_local_select_stress(false).await;
}

// endregion

// region `shared` stress tests

async fn test_shared_select_stress(with_default: bool) {
    const TRIES: usize = 1; // TODO
    const FIRST_SHIFT: usize = 12;
    const SECOND_SHIFT: usize = 21;
    const THIRD_SHIFT: usize = 30;
    const PAR_MULTIPLIER: usize = 6;
    const COUNT: usize = 50000;

    type ChanCreator = fn() -> Channel<usize>;

    fn full_bounded_chan_creator() -> Channel<usize> {
        Channel::bounded(COUNT)
    }

    fn one_bounded_chan_creator() -> Channel<usize> {
        Channel::bounded(1)
    }

    fn zero_bounded_chan_creator() -> Channel<usize> {
        Channel::bounded(0)
    }

    async fn stress_test_with_creators_select(
        with_default: bool,
        chan_1_creator: ChanCreator,
        chan_2_creator: ChanCreator,
        chan_3_creator: ChanCreator,
        chan_4_creator: ChanCreator,
    ) {
        let chan1 = Arc::new(chan_1_creator());
        let chan2 = Arc::new(chan_2_creator());
        let chan3 = Arc::new(chan_3_creator());
        let chan4 = Arc::new(chan_4_creator());
        let total_sent = Arc::new(AtomicUsize::new(0));
        let total_received = Arc::new(AtomicUsize::new(0));
        let wg = Arc::new(WaitGroup::new());

        for _ in 0..PAR_MULTIPLIER {
            {
                let wg_clone = wg.clone();
                let total_received_clone = total_received.clone();
                let chan1 = chan1.clone();
                let chan2 = chan2.clone();
                let chan3 = chan3.clone();
                let chan4 = chan4.clone();

                wg.inc().await;

                sched_future(async move {
                    let count = if with_default { COUNT } else { COUNT * 2 };

                    for i in 0..count {
                        let wg_clone2 = wg_clone.clone();
                        let total_received_clone2 = total_received_clone.clone();
                        let chan1_clone = chan1.clone();
                        let chan2_clone = chan2.clone();
                        let chan3_clone = chan3.clone();
                        let chan4_clone = chan4.clone();

                        wg_clone2.inc().await;

                        local_executor().spawn_shared(async move {
                            let received = if with_default {
                                let mut received;

                                loop {
                                    received = if i % 2 == 0 {
                                        select! {
                                            recv(&chan1_clone) -> received => Some(received)
                                            recv(&chan2_clone) -> received => Some(received)
                                            recv(&chan3_clone) -> received => Some(received)
                                            recv(&chan4_clone) -> received => Some(received)
                                            default => None
                                        }
                                    } else {
                                        select! {
                                            recv(&chan4_clone) -> received => Some(received)
                                            recv(&chan3_clone) -> received => Some(received)
                                            recv(&chan2_clone) -> received => Some(received)
                                            recv(&chan1_clone) -> received => Some(received)
                                            default => None
                                        }
                                    };

                                    if let Some(received) = received {
                                        break received;
                                    }

                                    yield_now().await;
                                }
                            } else if i % 2 == 0 {
                                select! {
                                    recv(&chan1_clone) -> received => received
                                    recv(&chan2_clone) -> received => received
                                    recv(&chan3_clone) -> received => received
                                    recv(&chan4_clone) -> received => received
                                }
                            } else {
                                select! {
                                    recv(&chan4_clone) -> received => received
                                    recv(&chan3_clone) -> received => received
                                    recv(&chan2_clone) -> received => received
                                    recv(&chan1_clone) -> received => received
                                }
                            };

                            total_received_clone2
                                .fetch_add(received.expect("failed to recv"), Relaxed);

                            wg_clone2.done().await;
                        });
                    }

                    wg_clone.done().await;
                });
            }

            {
                let wg_clone = wg.clone();
                let total_sent_clone = total_sent.clone();
                let chan1_clone = chan1.clone();
                let chan2_clone = chan2.clone();

                wg_clone.inc().await;

                sched_future(async move {
                    for i in 0..COUNT {
                        let wg_clone2 = wg_clone.clone();
                        let chan1_clone2 = chan1_clone.clone();
                        let chan2_clone2 = chan2_clone.clone();
                        let total_sent_clone2 = total_sent_clone.clone();

                        wg_clone2.inc().await;

                        local_executor().spawn_shared(async move {
                            if i % 2 == 0 {
                                chan1_clone2.send(i).await.expect("failed to send");

                                total_sent_clone2.fetch_add(i, Relaxed);
                            } else {
                                chan2_clone2
                                    .send(i << FIRST_SHIFT)
                                    .await
                                    .expect("failed to send");

                                total_sent_clone2.fetch_add(i << FIRST_SHIFT, Relaxed);
                            }

                            wg_clone2.done().await;
                        });
                    }

                    wg_clone.done().await;
                });
            }

            if !with_default {
                let wg_clone = wg.clone();
                let total_sent_clone = total_sent.clone();
                let chan3_clone = chan3.clone();
                let chan4_clone = chan4.clone();

                wg_clone.inc().await;

                sched_future(async move {
                    for i in 0..COUNT {
                        let wg_clone2 = wg_clone.clone();
                        let chan3_clone2 = chan3_clone.clone();
                        let chan4_clone2 = chan4_clone.clone();
                        let total_sent_clone2 = total_sent_clone.clone();

                        wg_clone2.inc().await;

                        local_executor().spawn_shared(async move {
                            let mut number_of_tries = 0;

                            loop {
                                if number_of_tries > COUNT * 30 {
                                    let stat3 = chan3_clone2.fullness_state().await;
                                    let stat4 = chan4_clone2.fullness_state().await;

                                    panic!(
                                        "Maybe try_send can't work with select. Too many tries. Stat1: {stat3:?} Stat2: {stat4:?}"
                                    );
                                };

                                let (res, n) = if i % 2 == 0 {
                                    let value = i << SECOND_SHIFT;

                                    (chan3_clone2.try_send(value), value)
                                } else {
                                    let value = i << THIRD_SHIFT;

                                    (chan4_clone2.try_send(value), value)
                                };

                                match res {
                                    Ok(()) => {
                                        total_sent_clone2.fetch_add(n, Relaxed);

                                        break;
                                    }
                                    Err(_) => yield_now().await,
                                }

                                number_of_tries += 1;
                            }

                            wg_clone2.done().await;
                        });
                    }

                    wg_clone.done().await;
                });
            }
        }

        wg.wait().await;

        assert_eq!(total_sent.load(Relaxed), total_received.load(Relaxed));
    }

    for _ in 0..TRIES {
        stress_test_with_creators_select(
            with_default,
            full_bounded_chan_creator,
            full_bounded_chan_creator,
            full_bounded_chan_creator,
            full_bounded_chan_creator,
        )
        .await;

        stress_test_with_creators_select(
            with_default,
            zero_bounded_chan_creator,
            zero_bounded_chan_creator,
            zero_bounded_chan_creator,
            zero_bounded_chan_creator,
        )
        .await;

        stress_test_with_creators_select(
            with_default,
            one_bounded_chan_creator,
            one_bounded_chan_creator,
            one_bounded_chan_creator,
            one_bounded_chan_creator,
        )
        .await;

        stress_test_with_creators_select(
            with_default,
            zero_bounded_chan_creator,
            zero_bounded_chan_creator,
            one_bounded_chan_creator,
            one_bounded_chan_creator,
        )
        .await;
    }
}

#[orengine::test::test_shared(timeout_ms = 10000, exclusive_in = "*")]
fn test_shared_select_stress_with_default() {
    test_shared_select_stress(true).await;
}

#[orengine::test::test_shared(timeout_ms = 10000, exclusive_in = "*")] // TODO 10000
fn test_shared_select_stress_without_default() {
    test_shared_select_stress(false).await;
}

// endregion


async fn test_bug_() {
    const TRIES: usize = 1;
    const FIRST_SHIFT: usize = 12;
    const SECOND_SHIFT: usize = 21;
    const THIRD_SHIFT: usize = 30;
    const PAR_MULTIPLIER: usize = 2;
    const COUNT: usize = 300000;

    type ChanCreator = fn() -> Channel<usize>;

    fn full_bounded_chan_creator() -> Channel<usize> {
        Channel::bounded(COUNT)
    }

    fn one_bounded_chan_creator() -> Channel<usize> {
        Channel::bounded(1)
    }

    fn zero_bounded_chan_creator() -> Channel<usize> {
        Channel::bounded(0)
    }

    async fn stress_test_with_creators_select(
        with_default: bool,
        chan_1_creator: ChanCreator,
        chan_2_creator: ChanCreator,
    ) {
        async fn spawn_receivers_without_default(
            chan1: Arc<Channel<usize>>,
            chan2: Arc<Channel<usize>>,
            total_received: Arc<AtomicUsize>,
            wg: Arc<WaitGroup>,
        ) {
            // TODO
            static IN: AtomicUsize = AtomicUsize::new(0);
            // TODO
            static OUT: AtomicUsize = AtomicUsize::new(0);

            println!("Recv executor has the id: {}", local_executor().id());

            wg.inc().await; // TODO 5 8 13 15

            for i in 0..COUNT {
                let wg_clone = wg.clone();
                let chan1_clone = chan1.clone();
                let chan2_clone = chan2.clone();
                let total_received_clone = total_received.clone();

                wg_clone.inc().await;

                local_executor().spawn_shared(async move {
                    let prev = IN.fetch_add(1, Relaxed);

                    if prev % 50_000 == 0 {
                        println!("Recv in {prev} from {}", COUNT * 2 * PAR_MULTIPLIER)
                    }

                    let received = if i % 2 == 0 {
                        select! {
                            recv(&chan1_clone) -> received => received
                            recv(&chan2_clone) -> received => received
                        }
                    } else {
                        select! {
                            recv(&chan2_clone) -> received => received
                            recv(&chan1_clone) -> received => received
                        }
                    };

                    let prev = OUT.fetch_add(1, Relaxed);

                    if prev % 50_000 == 0 {
                        println!("Recv out {prev} from {}", COUNT * 2 * PAR_MULTIPLIER)
                    }

                    total_received_clone.fetch_add(received.expect("failed to recv"), Relaxed);

                    wg_clone.done().await;
                });
            }

            wg.done().await;

            sync_wait_wg(&wg).await;
        }

        let chan1 = Arc::new(chan_1_creator());
        let chan2 = Arc::new(chan_2_creator());
        let total_sent = Arc::new(AtomicUsize::new(0));
        let total_received = Arc::new(AtomicUsize::new(0));
        let wg = Arc::new(WaitGroup::new());

        for _ in 0..PAR_MULTIPLIER {
            {
                let wg_clone = wg.clone();
                let chan1_clone = chan1.clone();
                let chan2_clone = chan2.clone();
                let total_received_clone = total_received.clone();

                sched_future(async move {
                    spawn_receivers_without_default(
                        chan1_clone,
                        chan2_clone,
                        total_received_clone,
                        wg_clone,
                    )
                    .await;
                });

                let wg_clone = wg.clone();
                let total_sent_clone = total_sent.clone();
                let chan1_clone = chan1.clone();

                // TODO
                static IN: AtomicUsize = AtomicUsize::new(0);

                IN.fetch_add(COUNT, Release);

                wg_clone.inc().await;

                sched_future(async move {
                    for i in 0..COUNT * 2 {
                        let wg_clone2 = wg_clone.clone();
                        let chan1_clone2 = chan1_clone.clone();
                        let total_sent_clone2 = total_sent_clone.clone();

                        wg_clone2.inc().await;

                        local_executor().spawn_shared(async move {
                            chan1_clone2.send(i).await.expect("failed to send");

                            total_sent_clone2.fetch_add(i, Relaxed);

                            let prev = IN.fetch_sub(1, Release);
                            if prev % COUNT == 1 {
                                println!("send ready");
                            }

                            wg_clone2.done().await;
                        });
                    }

                    wg_clone.done().await;

                    sync_wait_wg(&wg_clone).await;
                });
            }

            if !with_default {
                let wg_clone = wg.clone();
                let chan1_clone = chan1.clone();
                let chan2_clone = chan2.clone();
                let total_received_clone = total_received.clone();

                sched_future(async move {
                    spawn_receivers_without_default(
                        chan1_clone,
                        chan2_clone,
                        total_received_clone,
                        wg_clone,
                    )
                    .await;
                });

                let wg_clone = wg.clone();
                let total_sent_clone = total_sent.clone();
                let chan1_clone = chan1.clone();

                // // TODO
                // static IN: AtomicUsize = AtomicUsize::new(0);
                //
                // IN.fetch_add(COUNT, Release);
                //
                // wg_clone.inc().await;
                //
                // sched_future(async move {
                //     for i in 0..COUNT {
                //         let wg_clone2 = wg_clone.clone();
                //         let chan1_clone2 = chan1_clone.clone();
                //         let total_sent_clone2 = total_sent_clone.clone();
                //
                //         wg_clone2.inc().await;
                //
                //         local_executor().spawn_shared(async move {
                //             chan1_clone2.send(i).await.expect("failed to send");
                //
                //             total_sent_clone2.fetch_add(i, Relaxed);
                //
                //             let prev = IN.fetch_sub(1, Release);
                //             if prev % COUNT == 1 {
                //                 println!("send ready");
                //             }
                //
                //             wg_clone2.done().await;
                //         });
                //     }
                //
                //     wg_clone.done().await;
                //
                //     sync_wait_wg(&wg_clone).await;
                // });
            }
        }

        wg.wait().await;

        assert_eq!(total_sent.load(Relaxed), total_received.load(Relaxed));
    }

    for _ in 0..TRIES {
        stress_test_with_creators_select(
            false,
            zero_bounded_chan_creator,
            zero_bounded_chan_creator,
        )
        .await;
    }
}

// TODO r
#[orengine::test::test_shared(timeout_ms = 10000, exclusive_in = "*")]
fn test_bug() {
    test_bug_().await;
}
