// TODO
use crate as orengine;
use crate::sync::{
    AsyncChannel, AsyncReceiver, AsyncSender, Channel, LocalChannel,
};
use crate::{local_executor, sleep};
use orengine::select;
use std::rc::Rc;
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
        let ch3 = LocalChannel::<u32>::bounded(1);

        ch2.send(31).await.expect("failed to send");

        let a = select! {
            recv(&ch1) -> var => var.unwrap()
            recv(&ch2) -> var => var.unwrap()
            send(&ch3, 20) -> _var => 1
            default => 4
        };

        assert_eq!(a, RES, "non-blocking recv assertion failed");
    }

    // non-blocking send success
    {
        const RES: u32 = 23;

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

        const SENT: u32 = 61;

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
        let ch3 = LocalChannel::<u32>::bounded(1);

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
                    _ => panic!("non-blocking send with error failed"),
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
        let ch3 = LocalChannel::<u32>::bounded(1);

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

        const SENT: u32 = 61;

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
        let ch3 = LocalChannel::<u32>::bounded(1);

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
                    _ => panic!("non-blocking without default send with error failed"),
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
        let ch3 = Channel::<u32>::bounded(1);

        ch2.send(31).await.expect("failed to send");

        let a = select! {
            recv(&ch1) -> var => var.unwrap()
            recv(&ch2) -> var => var.unwrap()
            send(&ch3, 20) -> _var => 1
            default => 4
        };

        assert_eq!(a, RES, "non-blocking recv assertion failed");
    }

    // non-blocking send success
    {
        const RES: u32 = 23;

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

        const SENT: u32 = 61;

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
        let ch3 = Channel::<u32>::bounded(1);

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
                    _ => panic!("non-blocking send with error failed"),
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

        const SENT: u32 = 61;

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
        let ch3 = Channel::<u32>::bounded(1);

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
                    _ => panic!("non-blocking without default send with error failed"),
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

// region `shared` stress tests

// TODO
// #[orengine::test::test_shared]
// fn test_shared_select_stress() {
//     const TRIES: usize = 25;
//     const FIRST_SHIFT: usize = 12;
//     const SECOND_SHIFT: usize = 30;
//     const N: usize = 10_000;
//     const PAR_MULTIPLIER: usize = 3;
//     const COUNT: usize = N / PAR_MULTIPLIER;
//
//     type ChanCreator = fn() -> Channel<usize>;
//
//     fn full_bounded_chan_creator() -> Channel<usize> {
//         Channel::bounded(N)
//     }
//
//     async fn stress_test_with_creators(
//         select_chan_1_creator: ChanCreator,
//         select_chan_2_creator: ChanCreator,
//         select_chan_3_creator: ChanCreator,
//         non_select_chan_1_creator: ChanCreator,
//         non_select_chan_2_creator: ChanCreator,
//         non_select_chan_3_creator: ChanCreator,
//     ) {
//         let select_chan_1 = Arc::new(select_chan_1_creator());
//         let select_chan_2 = Arc::new(select_chan_2_creator());
//         let select_chan_3 = Arc::new(select_chan_3_creator());
//         let non_select_chan_1 = Arc::new(non_select_chan_1_creator());
//         let non_select_chan_2 = Arc::new(non_select_chan_2_creator());
//         let non_select_chan_3 = Arc::new(non_select_chan_3_creator());
//         let total_sent = Arc::new(AtomicUsize::new(0));
//         let total_received = Arc::new(AtomicUsize::new(0));
//         let wg = Arc::new(WaitGroup::new());
//
//         for _ in 0..PAR_MULTIPLIER {
//             let wg_clone = wg.clone();
//             let total_sent_clone = total_sent.clone();
//             let total_received_clone = total_received.clone();
//             let select_chan_1 = select_chan_1.clone();
//             let select_chan_2 = select_chan_2.clone();
//             let select_chan_3 = select_chan_3.clone();
//             let non_select_chan_1 = non_select_chan_1.clone();
//             let non_select_chan_2 = non_select_chan_2.clone();
//             let non_select_chan_3 = non_select_chan_3.clone();
//
//             wg.inc();
//
//             sched_future_to_another_thread(async move {
//                 for i in 0..COUNT {
//                     if i % 2 == 0 {
//                         let received = select! {
//                             recv(&select_chan_1) -> received => received
//                             recv(&select_chan_2) -> received => received
//                             recv(&select_chan_3) -> received => received
//                         };
//
//                         total_received_clone.fetch_add(received.expect("failed to recv"), Relaxed);
//                     } else {
//                         let sent = select! {
//                             send(&select_chan_1, i) -> result => result.map(|()| i)
//                             send(&select_chan_2, i << FIRST_SHIFT) -> result => result.map(|()| i << FIRST_SHIFT)
//                             send(&select_chan_3, i << SECOND_SHIFT) -> result => result.map(|()| i << SECOND_SHIFT)
//                         };
//
//                         total_sent_clone.fetch_add(sent.expect("failed to send"), Relaxed);
//                     }
//                 }
//
//                 wg_clone.done();
//             });
//
//             let wg_clone = wg.clone();
//             let total_sent_clone = total_sent.clone();
//             let total_received_clone = total_received.clone();
//
//             wg.inc();
//
//             sched_future_to_another_thread(async move {
//                 for i in 0..COUNT {
//                     let total_received_clone2 = total_received_clone.clone();
//
//                     if i % 2 == 0 {
//                         local_executor().spawn_shared(async {
//                             total_received_clone2.fetch_add(
//                                 non_select_chan_1.recv().await.expect("failed to recv"),
//                                 Relaxed,
//                             );
//                         });
//
//                         let total_received_clone2 = total_received_clone.clone();
//
//                         local_executor().spawn_shared(async {
//                             total_received_clone2.fetch_add(
//                                 non_select_chan_2.recv().await.expect("failed to recv"),
//                                 Relaxed,
//                             );
//                         });
//                     } else {
//                         local_executor().spawn_shared(async {
//                             non_select_chan_1.send(i).await.expect("failed to send");
//
//                             total_received_clone2.fetch_add(i, Relaxed);
//                         });
//
//                         let total_sent_clone2 = total_sent_clone.clone();
//
//                         local_executor().spawn_shared(async {
//                             non_select_chan_2
//                                 .send(i << FIRST_SHIFT)
//                                 .await
//                                 .expect("failed to send");
//
//                             total_sent_clone2.fetch_add(i << FIRST_SHIFT, Relaxed);
//                         });
//                     }
//                 }
//
//                 wg_clone.done();
//             });
//
//             let wg_clone = wg.clone();
//             let total_sent_clone = total_sent.clone();
//             let total_received_clone = total_received.clone();
//
//             wg.inc();
//
//             local_executor().spawn_shared(async move {
//                 for i in 0..COUNT {
//                     if i % 2 == 0 {
//                         loop {
//                             match non_select_chan_3.try_recv() {
//                                 Ok(received) => {
//                                     total_received_clone.fetch_add(received, Relaxed);
//                                     break;
//                                 }
//                                 Err(_) => yield_now().await,
//                             }
//                         }
//                     } else {
//                         loop {
//                             match non_select_chan_3.try_send(i << SECOND_SHIFT) {
//                                 Ok(()) => {
//                                     total_sent_clone.fetch_add(i << SECOND_SHIFT, Relaxed);
//                                     break;
//                                 }
//                                 Err(_) => yield_now().await,
//                             }
//                         }
//                     }
//                 }
//
//                 wg_clone.done();
//             });
//         }
//
//         wg.wait().await;
//
//         assert_eq!(total_sent.load(Relaxed), total_received.load(Relaxed));
//         assert!(total_sent.load(Relaxed) > (N / 2) * (COUNT / 2) * 3 * PAR_MULTIPLIER); // TODO
//     }
//
//     for _ in 0..TRIES {
//         stress_test_with_creators(
//             full_bounded_chan_creator,
//             full_bounded_chan_creator,
//             full_bounded_chan_creator,
//             full_bounded_chan_creator,
//             full_bounded_chan_creator,
//             full_bounded_chan_creator,
//         )
//         .await;
//     }
// }

// endregion
