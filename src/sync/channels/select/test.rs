// TODO
use crate as orengine;
use crate::sync::{
    AsyncChannel, AsyncReceiver, AsyncSender, AsyncWaitGroup, Channel, LocalChannel,
    LocalWaitGroup, SendErr, WaitGroup,
};
use crate::test::sched_future_to_another_thread;
use crate::{local_executor, sleep, yield_now, Local};
use orengine::select;
use std::rc::Rc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use std::time::Duration;

// region `local` tests

#[orengine::test::test_local]
fn test_local_select_with_default() {
    // default
    {
        let ch1 = LocalChannel::<u32>::bounded(1);
        let ch2 = LocalChannel::<u32>::bounded(1);
        let ch3 = LocalChannel::<u32>::bounded(0);

        let a = select! {
            recv(&ch2) -> _var => 1
            recv(&ch1) -> _var => 4
            send(&ch3, 20) -> res => {
                if let Err(_e) = res {
                    2
                } else {
                    3
                }
            }
            default => 5
        };

        assert_eq!(a, 5, "default assertion failed");
    }

    // non-blocking recv success
    {
        const RES: u32 = 31;

        let ch1 = LocalChannel::<u32>::bounded(1);
        let ch2 = LocalChannel::<u32>::bounded(1);

        ch2.send(31).await.expect("failed to send");

        let a = select! {
            recv(&ch1) -> var => var.unwrap()
            recv(&ch2) -> var => var.unwrap()
            send(&ch2, 20) -> _var => 1
            default => 4
        };

        assert_eq!(a, RES, "non-blocking recv assertion failed");
    }

    // non-blocking send success
    {
        const RES: u32 = 23;
        const SENT: u32 = 61;

        let chan = Rc::new(LocalChannel::<u32>::bounded(1));
        let chan_clone = chan.clone();

        local_executor().spawn_local(async {
            let ch2 = LocalChannel::<u32>::bounded(1);
            let ch3 = LocalChannel::<u32>::bounded(1);
            select! {
                recv(&ch2) -> _var => ()
                recv(&ch3) -> _var => ()
                send(&chan_clone, RES) -> res => {
                    res.expect("channel is closed");
                }
                default => ()
            }
        });

        assert_eq!(chan.recv().await.expect("failed to receive"), RES);

        let ch1 = LocalChannel::<u32>::bounded(1);
        let ch2 = LocalChannel::<u32>::bounded(1);
        let a = select! {
            recv(&ch1) -> var => var.unwrap()
            recv(&ch2) -> var => var.unwrap()
            send(&chan, 20) -> _var => SENT
            default => 4
        };

        assert_eq!(a, SENT, "non-blocking recv assertion failed");
    }

    // non-blocking recv error
    {
        let ch1 = LocalChannel::<u32>::bounded(1);
        let ch2 = LocalChannel::<u32>::bounded(1);
        let ch3 = LocalChannel::<u32>::bounded(0);

        ch2.close().await;

        select! {
            recv(&ch1) -> _var => panic!("non-blocking recv with error failed")
            recv(&ch2) -> var => match var {
                Ok(_) => panic!("non-blocking recv with error failed"),
                Err(e) => assert!(matches!(e, RecvErr::Closed), "non-blocking recv with error failed"),
            }
            send(&ch3, 20) -> _var => panic!("non-blocking recv with error failed")
            default => panic!("non-blocking recv with error failed")
        }
    }

    // non-blocking send error
    {
        let ch1 = LocalChannel::<u32>::bounded(1);
        let ch2 = LocalChannel::<u32>::bounded(1);
        let ch3 = LocalChannel::<u32>::bounded(1);

        ch3.close().await;

        select! {
            recv(&ch1) -> _var => panic!("non-blocking send with error failed")
            recv(&ch2) -> _var => panic!("non-blocking send with error failed")
            send(&ch3, 20) -> var => match var {
                Ok(()) => panic!("non-blocking send with error failed"),
                Err(e) => match e {
                    SendErr::Closed(20) => (),
                    SendErr::Closed(_) => panic!("non-blocking send with error failed"),
                }
            }
            default => panic!("non-blocking send with error failed")
        }
    }
}

#[orengine::test::test_local]
fn test_local_select_without_default_non_blocking() {
    // non-blocking recv success
    {
        const RES: u32 = 31;

        let ch1 = LocalChannel::<u32>::bounded(1);
        let ch2 = LocalChannel::<u32>::bounded(1);
        let ch3 = LocalChannel::<u32>::bounded(0);

        ch2.send(31).await.expect("failed to send");

        let a = select! {
            recv(&ch1) -> var => var.unwrap()
            recv(&ch2) -> var => var.unwrap()
            send(&ch3, 20) -> _var => 1
        };

        assert_eq!(a, RES, "non-blocking without default recv assertion failed");
    }

    // non-blocking send success
    {
        const RES: u32 = 23;
        const SENT: u32 = 61;

        let chan = Rc::new(LocalChannel::<u32>::bounded(1));
        let chan_clone = chan.clone();

        local_executor().spawn_local(async {
            let ch2 = LocalChannel::<u32>::bounded(1);
            let ch3 = LocalChannel::<u32>::bounded(1);
            select! {
                recv(&ch2) -> _var => ()
                recv(&ch3) -> _var => ()
                send(&chan_clone, RES) -> res => {
                    res.expect("channel is closed");
                }
                default => ()
            }
        });

        assert_eq!(chan.recv().await.expect("failed to receive"), RES);

        let ch1 = LocalChannel::<u32>::bounded(1);
        let ch2 = LocalChannel::<u32>::bounded(1);
        let a = select! {
            recv(&ch1) -> var => var.unwrap()
            recv(&ch2) -> var => var.unwrap()
            send(&chan, 20) -> _var => SENT
        };

        assert_eq!(
            a, SENT,
            "non-blocking without default recv assertion failed"
        );
    }

    // non-blocking recv error
    {
        let ch1 = LocalChannel::<u32>::bounded(1);
        let ch2 = LocalChannel::<u32>::bounded(1);
        let ch3 = LocalChannel::<u32>::bounded(0);

        ch2.close().await;

        select! {
            recv(&ch1) -> _var => panic!("non-blocking without default recv with error failed")
            recv(&ch2) -> var => match var {
                Ok(_) => panic!("non-blocking without default recv with error failed"),
                Err(e) => assert!(matches!(e, RecvErr::Closed), "non-blocking recv with error failed"),
            }
            send(&ch3, 20) -> _var => panic!("non-blocking without default recv with error failed")
        }
    }

    // non-blocking send error
    {
        let ch1 = LocalChannel::<u32>::bounded(1);
        let ch2 = LocalChannel::<u32>::bounded(1);
        let ch3 = LocalChannel::<u32>::bounded(1);

        ch3.close().await;

        select! {
            recv(&ch1) -> _var => panic!("non-blocking without default send with error failed")
            recv(&ch2) -> _var => panic!("non-blocking without default send with error failed")
            send(&ch3, 20) -> var => match var {
                Ok(()) => panic!("non-blocking without default send with error failed"),
                Err(e) => match e {
                    SendErr::Closed(20) => (),
                    SendErr::Closed(_) => panic!("non-blocking without default send with error failed"),
                }
            }
        }
    }
}

#[orengine::test::test_local]
fn test_local_select_without_default_blocking() {
    // blocking recv success
    {
        const RES: u32 = 39;

        let ch1 = LocalChannel::<u32>::bounded(1);
        let ch2 = Rc::new(LocalChannel::<u32>::bounded(1));
        let ch2_clone = ch2.clone();
        let ch3 = LocalChannel::<u32>::bounded(0);

        local_executor().spawn_local(async move {
            sleep(Duration::from_micros(100)).await;
            ch2_clone.send(RES).await.expect("failed to send");
        });

        let a = select! {
            recv(&ch1) -> var => var.unwrap()
            recv(&ch2) -> var => var.unwrap()
            send(&ch3, 20) -> _var => 1
        };

        assert_eq!(a, RES, "blocking recv assertion failed");
    }

    // blocking send success
    {
        const RES: u32 = 29;

        let ch3 = Rc::new(LocalChannel::<u32>::bounded(0));
        let ch3_clone = ch3.clone();

        local_executor().spawn_local(async move {
            const SENT: u32 = 131;

            let ch1 = LocalChannel::<u32>::bounded(1);
            let ch2 = LocalChannel::<u32>::bounded(1);

            sleep(Duration::from_micros(100)).await;

            let a = select! {
                recv(&ch1) -> var => var.unwrap()
                recv(&ch2) -> var => var.unwrap()
                send(&ch3_clone, RES) -> _var => SENT
            };

            assert_eq!(a, SENT, "blocking send assertion failed");
        });

        assert_eq!(
            ch3.recv().await.expect("failed to send"),
            RES,
            "blocking send assertion failed"
        );
    }

    // blocking recv err
    {
        let ch1 = LocalChannel::<u32>::bounded(1);
        let ch2 = Rc::new(LocalChannel::<u32>::bounded(1));
        let ch2_clone = ch2.clone();
        let ch3 = LocalChannel::<u32>::bounded(0);

        local_executor().spawn_local(async move {
            sleep(Duration::from_micros(100)).await;

            ch2_clone.close().await;
        });

        select! {
            recv(&ch1) -> _var => panic!("blocking recv with error failed")
            recv(&ch2) -> var => assert!(var.is_err(), "blocking recv with error failed")
            send(&ch3, 20) -> _var => panic!("blocking recv with error failed")
        }
    }

    // blocking send err
    {
        let ch1 = LocalChannel::<u32>::bounded(1);
        let ch2 = LocalChannel::<u32>::bounded(1);
        let ch3 = Rc::new(LocalChannel::<u32>::bounded(0));
        let ch3_clone = ch3.clone();

        local_executor().spawn_local(async move {
            sleep(Duration::from_micros(100)).await;

            ch3_clone.close().await;
        });

        select! {
            recv(&ch1) -> _var => panic!("blocking send with error failed")
            recv(&ch2) -> _var => panic!("blocking send with error failed")
            send(&ch3, 30) -> var => match var {
                Err(SendErr::Closed(30)) => (),
                _ => panic!("blocking send with error failed"),
            }
        }
    }
}

// endregion

// region `shared` not stress tests

#[orengine::test::test_shared]
fn test_shared_select_with_default() {
    // default
    {
        let ch1 = Channel::<u32>::bounded(1);
        let ch2 = Channel::<u32>::bounded(1);
        let ch3 = Channel::<u32>::bounded(0);

        let a = select! {
            recv(&ch2) -> _var => 1
            recv(&ch1) -> _var => 4
            send(&ch3, 20) -> res => {
                if let Err(_e) = res {
                    2
                } else {
                    3
                }
            }
            default => 5
        };

        assert_eq!(a, 5, "default assertion failed");
    }

    // non-blocking recv success
    {
        const RES: u32 = 31;

        let ch1 = Channel::<u32>::bounded(1);
        let ch2 = Channel::<u32>::bounded(1);

        ch2.send(31).await.expect("failed to send");

        let a = select! {
            recv(&ch1) -> var => var.unwrap()
            recv(&ch2) -> var => var.unwrap()
            send(&ch2, 20) -> _var => 1
            default => 4
        };

        assert_eq!(a, RES, "non-blocking recv assertion failed");
    }

    // non-blocking send success
    {
        const RES: u32 = 23;
        const SENT: u32 = 61;

        let chan = Arc::new(Channel::<u32>::bounded(1));
        let chan_clone = chan.clone();

        local_executor().spawn_shared(async {
            let ch2 = Channel::<u32>::bounded(1);
            let ch3 = Channel::<u32>::bounded(1);
            select! {
                recv(&ch2) -> _var => ()
                recv(&ch3) -> _var => ()
                send(&chan_clone, RES) -> res => {
                    res.expect("channel is closed");
                }
                default => ()
            }
        });

        assert_eq!(chan.recv().await.expect("failed to receive"), RES);

        let ch1 = Channel::<u32>::bounded(1);
        let ch2 = Channel::<u32>::bounded(1);
        let a = select! {
            recv(&ch1) -> var => var.unwrap()
            recv(&ch2) -> var => var.unwrap()
            send(&chan, 20) -> _var => SENT
            default => 4
        };

        assert_eq!(a, SENT, "non-blocking recv assertion failed");
    }

    // non-blocking recv error
    {
        let ch1 = Channel::<u32>::bounded(1);
        let ch2 = Channel::<u32>::bounded(1);
        let ch3 = Channel::<u32>::bounded(0);

        ch2.close().await;

        select! {
            recv(&ch1) -> _var => panic!("non-blocking recv with error failed")
            recv(&ch2) -> var => match var {
                Ok(_) => panic!("non-blocking recv with error failed"),
                Err(e) => assert!(matches!(e, RecvErr::Closed), "non-blocking recv with error failed"),
            }
            send(&ch3, 20) -> _var => panic!("non-blocking recv with error failed")
            default => panic!("non-blocking recv with error failed")
        }
    }

    // non-blocking send error
    {
        let ch1 = Channel::<u32>::bounded(1);
        let ch2 = Channel::<u32>::bounded(1);
        let ch3 = Channel::<u32>::bounded(1);

        ch3.close().await;

        select! {
            recv(&ch1) -> _var => panic!("non-blocking send with error failed")
            recv(&ch2) -> _var => panic!("non-blocking send with error failed")
            send(&ch3, 20) -> var => match var {
                Ok(()) => panic!("non-blocking send with error failed"),
                Err(e) => match e {
                    SendErr::Closed(20) => (),
                    SendErr::Closed(_) => panic!("non-blocking send with error failed"),
                }
            }
            default => panic!("non-blocking send with error failed")
        }
    }
}

#[orengine::test::test_shared]
fn test_shared_select_without_default_non_blocking() {
    // non-blocking recv success
    {
        const RES: u32 = 31;

        let ch1 = Channel::<u32>::bounded(1);
        let ch2 = Channel::<u32>::bounded(1);
        let ch3 = Channel::<u32>::bounded(1);

        ch2.send(31).await.expect("failed to send");

        let a = select! {
            recv(&ch1) -> var => var.unwrap()
            recv(&ch2) -> var => var.unwrap()
            send(&ch3, 20) -> _var => 1
        };

        assert_eq!(a, RES, "non-blocking without default recv assertion failed");
    }

    // non-blocking send success
    {
        const RES: u32 = 23;
        const SENT: u32 = 61;

        let chan = Arc::new(Channel::<u32>::bounded(1));
        let chan_clone = chan.clone();

        local_executor().spawn_shared(async {
            let ch2 = Channel::<u32>::bounded(1);
            let ch3 = Channel::<u32>::bounded(1);
            select! {
                recv(&ch2) -> _var => ()
                recv(&ch3) -> _var => ()
                send(&chan_clone, RES) -> res => {
                    res.expect("channel is closed");
                }
                default => ()
            }
        });

        assert_eq!(chan.recv().await.expect("failed to receive"), RES);

        let ch1 = Channel::<u32>::bounded(1);
        let ch2 = Channel::<u32>::bounded(1);
        let a = select! {
            recv(&ch1) -> var => var.unwrap()
            recv(&ch2) -> var => var.unwrap()
            send(&chan, 20) -> _var => SENT
        };

        assert_eq!(
            a, SENT,
            "non-blocking without default recv assertion failed"
        );
    }

    // non-blocking recv error
    {
        let ch1 = Channel::<u32>::bounded(1);
        let ch2 = Channel::<u32>::bounded(1);
        let ch3 = Channel::<u32>::bounded(0);

        ch2.close().await;

        select! {
            recv(&ch1) -> _var => panic!("non-blocking without default recv with error failed")
            recv(&ch2) -> var => match var {
                Ok(_) => panic!("non-blocking without default recv with error failed"),
                Err(e) => assert!(matches!(e, RecvErr::Closed), "non-blocking recv with error failed"),
            }
            send(&ch3, 20) -> _var => panic!("non-blocking without default recv with error failed")
        }
    }

    // non-blocking send error
    {
        let ch1 = Channel::<u32>::bounded(1);
        let ch2 = Channel::<u32>::bounded(1);
        let ch3 = Channel::<u32>::bounded(1);

        ch3.close().await;

        select! {
            recv(&ch1) -> _var => panic!("non-blocking without default send with error failed")
            recv(&ch2) -> _var => panic!("non-blocking without default send with error failed")
            send(&ch3, 20) -> var => match var {
                Ok(()) => panic!("non-blocking without default send with error failed"),
                Err(e) => match e {
                    SendErr::Closed(20) => (),
                    SendErr::Closed(_) => panic!("non-blocking without default send with error failed"),
                }
            }
        }
    }
}

#[orengine::test::test_shared]
fn test_shared_select_without_default_blocking() {
    // blocking recv success
    {
        const RES: u32 = 39;

        let ch1 = Channel::<u32>::bounded(1);
        let ch2 = Arc::new(Channel::<u32>::bounded(1));
        let ch2_clone = ch2.clone();
        let ch3 = Channel::<u32>::bounded(0);

        local_executor().spawn_shared(async move {
            sleep(Duration::from_micros(100)).await;
            ch2_clone.send(RES).await.expect("failed to send");
        });

        let a = select! {
            recv(&ch1) -> var => var.unwrap()
            recv(&ch2) -> var => var.unwrap()
            send(&ch3, 20) -> _var => 1
        };

        assert_eq!(a, RES, "blocking recv assertion failed");
    }

    // blocking send success
    {
        const RES: u32 = 29;

        let ch3 = Arc::new(Channel::<u32>::bounded(0));
        let ch3_clone = ch3.clone();

        local_executor().spawn_shared(async move {
            const SENT: u32 = 131;

            let ch1 = Channel::<u32>::bounded(1);
            let ch2 = Channel::<u32>::bounded(1);

            sleep(Duration::from_micros(100)).await;

            let a = select! {
                recv(&ch1) -> var => var.unwrap()
                recv(&ch2) -> var => var.unwrap()
                send(&ch3_clone, RES) -> _var => SENT
            };

            assert_eq!(a, SENT, "blocking send assertion failed");
        });

        assert_eq!(
            ch3.recv().await.expect("failed to send"),
            RES,
            "blocking send assertion failed"
        );
    }

    // blocking recv err
    {
        let ch1 = Channel::<u32>::bounded(1);
        let ch2 = Arc::new(Channel::<u32>::bounded(1));
        let ch2_clone = ch2.clone();
        let ch3 = Channel::<u32>::bounded(0);

        local_executor().spawn_shared(async move {
            sleep(Duration::from_micros(100)).await;

            ch2_clone.close().await;
        });

        select! {
            recv(&ch1) -> _var => panic!("blocking recv with error failed")
            recv(&ch2) -> var => assert!(var.is_err(), "blocking recv with error failed")
            send(&ch3, 20) -> _var => panic!("blocking recv with error failed")
        }
    }

    // blocking send err
    {
        let ch1 = Channel::<u32>::bounded(1);
        let ch2 = Channel::<u32>::bounded(1);
        let ch3 = Arc::new(Channel::<u32>::bounded(0));
        let ch3_clone = ch3.clone();

        local_executor().spawn_shared(async move {
            sleep(Duration::from_micros(100)).await;

            ch3_clone.close().await;
        });

        select! {
            recv(&ch1) -> _var => panic!("blocking send with error failed")
            recv(&ch2) -> _var => panic!("blocking send with error failed")
            send(&ch3, 30) -> var => match var {
                Err(SendErr::Closed(30)) => (),
                _ => panic!("blocking send with error failed"),
            }
        }
    }
}

// endregion

// region `local` stress tests

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

                wg.inc();

                local_executor().spawn_local(async move {
                    let count = if with_default { COUNT } else { COUNT * 2 };

                    for i in 0..count {
                        let wg_clone2 = wg_clone.clone();
                        let total_received_clone2 = total_received_clone.clone();
                        let chan1_clone = chan1.clone();
                        let chan2_clone = chan2.clone();
                        let chan3_clone = chan3.clone();
                        let chan4_clone = chan4.clone();

                        wg_clone2.inc();

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

                            wg_clone2.done();
                        });
                    }

                    wg_clone.done();
                });
            }

            {
                let wg_clone = wg.clone();
                let total_sent_clone = total_sent.clone();
                let chan1_clone = chan1.clone();
                let chan2_clone = chan2.clone();

                wg_clone.inc();

                local_executor().spawn_local(async move {
                    for i in 0..COUNT {
                        let wg_clone2 = wg_clone.clone();
                        let chan1_clone2 = chan1_clone.clone();
                        let chan2_clone2 = chan2_clone.clone();
                        let total_sent_clone2 = total_sent_clone.clone();

                        wg_clone2.inc();

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

                            wg_clone2.done();
                        });
                    }

                    wg_clone.done();
                });
            }

            if !with_default {
                let wg_clone = wg.clone();
                let total_sent_clone = total_sent.clone();
                let chan3_clone = chan3.clone();
                let chan4_clone = chan4.clone();

                wg_clone.inc();

                local_executor().spawn_local(async move {
                    for i in 0..COUNT {
                        let wg_clone2 = wg_clone.clone();
                        let chan3_clone2 = chan3_clone.clone();
                        let chan4_clone2 = chan4_clone.clone();
                        let total_sent_clone2 = total_sent_clone.clone();

                        wg_clone2.inc();

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

                            wg_clone2.done();
                        });
                    }

                    wg_clone.done();
                });
            }
        }

        wg.wait().await;

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

#[orengine::test::test_local]
fn test_local_select_stress_with_default() {
    test_local_select_stress(true).await;
}

#[orengine::test::test_local]
fn test_local_select_stress_without_default() {
    test_local_select_stress(false).await;
}

// endregion

// region `shared` stress tests

async fn test_shared_select_stress(with_default: bool) {
    #[cfg(debug_assertions)]
    const TRIES: usize = 3;
    #[cfg(not(debug_assertions))]
    const TRIES: usize = 10;
    const FIRST_SHIFT: usize = 12;
    const SECOND_SHIFT: usize = 21;
    const THIRD_SHIFT: usize = 30;
    const PAR_MULTIPLIER: usize = 3;
    const COUNT: usize = 1000;

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

                wg.inc();

                sched_future_to_another_thread(async move {
                    let count = if with_default { COUNT } else { COUNT * 2 };

                    for i in 0..count {
                        let wg_clone2 = wg_clone.clone();
                        let total_received_clone2 = total_received_clone.clone();
                        let chan1_clone = chan1.clone();
                        let chan2_clone = chan2.clone();
                        let chan3_clone = chan3.clone();
                        let chan4_clone = chan4.clone();

                        wg_clone2.inc();

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

                            wg_clone2.done();
                        });
                    }

                    wg_clone.done();
                });
            }

            {
                let wg_clone = wg.clone();
                let total_sent_clone = total_sent.clone();
                let chan1_clone = chan1.clone();
                let chan2_clone = chan2.clone();

                wg_clone.inc();

                sched_future_to_another_thread(async move {
                    for i in 0..COUNT {
                        let wg_clone2 = wg_clone.clone();
                        let chan1_clone2 = chan1_clone.clone();
                        let chan2_clone2 = chan2_clone.clone();
                        let total_sent_clone2 = total_sent_clone.clone();

                        wg_clone2.inc();

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

                            wg_clone2.done();
                        });
                    }

                    wg_clone.done();
                });
            }

            if !with_default {
                let wg_clone = wg.clone();
                let total_sent_clone = total_sent.clone();
                let chan3_clone = chan3.clone();
                let chan4_clone = chan4.clone();

                wg_clone.inc();

                sched_future_to_another_thread(async move {
                    for i in 0..COUNT {
                        let wg_clone2 = wg_clone.clone();
                        let chan3_clone2 = chan3_clone.clone();
                        let chan4_clone2 = chan4_clone.clone();
                        let total_sent_clone2 = total_sent_clone.clone();

                        wg_clone2.inc();

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

                            wg_clone2.done();
                        });
                    }

                    wg_clone.done();
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

#[orengine::test::test_shared]
fn test_shared_select_stress_with_default() {
    test_shared_select_stress(true).await;
}

#[orengine::test::test_shared]
fn test_shared_select_stress_without_default() {
    test_shared_select_stress(false).await;
}

// endregion

#[orengine::test::test_local]
fn test_select_one_channel_recv() {
    let chan = Rc::new(LocalChannel::bounded(0));
    let chan_clone = chan.clone();

    local_executor().spawn_local(async move {
        sleep(Duration::from_micros(10)).await;

        chan_clone.send(1).await.expect("failed to send");
    });

    let res: i32 = select! {
        recv(&chan) -> res => res
    }
    .expect("failed to recv");

    assert_eq!(res, 1);

    let chan_clone = chan.clone();

    local_executor().spawn_local(async move {
        sleep(Duration::from_micros(10)).await;

        chan_clone.close().await;
    });

    let res = select! {
        recv(&chan) -> res => res
    };

    res.unwrap_err();
}

#[orengine::test::test_local]
fn test_select_one_channel_send() {
    let chan = Rc::new(LocalChannel::bounded(1));
    let res = select! {
        send(&chan, 1) -> res => res
    };

    res.unwrap();

    let chan_clone = chan.clone();

    local_executor().spawn_local(async move {
        sleep(Duration::from_micros(10)).await;

        chan_clone.close().await;
    });

    let res = select! {
        send(&chan, 1) -> res => res
    }
    .unwrap_err();

    match res {
        SendErr::Closed(val) => assert_eq!(val, 1),
    }
}

#[test]
fn test_select_one_channel_try_recv() {
    let chan = LocalChannel::bounded(1);

    let res = select! {
        recv(&chan) -> res => res
        default => Ok(1)
    };

    assert_eq!(res.unwrap(), 1);

    chan.try_send(2).expect("failed to send");

    let res = select! {
        recv(&chan) -> res => res
        default => Ok(1)
    };

    assert_eq!(res.unwrap(), 2);
}

#[test]
fn test_select_one_channel_try_send() {
    let chan = LocalChannel::bounded(0);

    let res = select! {
        send(&chan, 1) -> _res => 2
        default => 1
    };

    assert_eq!(res, 1);

    let chan = LocalChannel::bounded(1);

    let res = select! {
        send(&chan, 1) -> _res => 3
        default => 1
    };

    assert_eq!(res, 3);
}
