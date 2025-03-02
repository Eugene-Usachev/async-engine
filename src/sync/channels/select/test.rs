// TODO
use crate as orengine;
use crate::sync::{local_scope, AsyncChannel, AsyncReceiver, AsyncSender, LocalChannel};
use crate::{local_executor, sleep};
use orengine_macros::select;
use std::rc::Rc;
use std::time::Duration;

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
                if let Err(e) = res {
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

        let chan = LocalChannel::<u32>::bounded(1);

        local_scope(|scope| async {
            scope.spawn(async {
                let ch2 = LocalChannel::<u32>::bounded(1);
                let ch3 = LocalChannel::<u32>::bounded(1);
                select! {
                    recv(&ch2) -> _var => ()
                    recv(&ch3) -> _var => ()
                    send(&chan, RES) -> res => {
                        res.expect("channel is closed");
                    }
                    default => ()
                }
            });
        })
        .await;

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
        };
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
        };
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

        let chan = LocalChannel::<u32>::bounded(1);

        local_scope(|scope| async {
            scope.spawn(async {
                let ch2 = LocalChannel::<u32>::bounded(1);
                let ch3 = LocalChannel::<u32>::bounded(1);
                select! {
                    recv(&ch2) -> _var => ()
                    recv(&ch3) -> _var => ()
                    send(&chan, RES) -> res => {
                        res.expect("channel is closed");
                    }
                    default => ()
                }
            });
        })
        .await;

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
        };
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
        };
    }
}

#[orengine::test::test_local]
fn test_local_select_without_default_blocking() {
    // blocking recv success
    {
        const RES: u32 = 39;

        let ch1 = LocalChannel::<u32>::bounded(1);
        let ch2 = LocalChannel::<u32>::bounded(1);
        let ch3 = LocalChannel::<u32>::bounded(0);

        local_scope(|scope| async {
            scope.spawn(async {
                sleep(Duration::from_micros(100)).await;
                ch2.send(RES).await.expect("failed to send");
            });

            let a = select! {
                recv(&ch1) -> var => var.unwrap()
                recv(&ch2) -> var => var.unwrap()
                send(&ch3, 20) -> _var => 1
            };

            assert_eq!(a, RES, "blocking recv assertion failed");
        })
        .await;
    }

    // blocking send success
    {
        const RES: u32 = 29;

        let ch1 = Rc::new(LocalChannel::<u32>::bounded(1));
        let ch1_clone = ch1.clone();
        let ch2 = LocalChannel::<u32>::bounded(1);
        let ch3 = LocalChannel::<u32>::bounded(1);

        local_executor().spawn_local(async {
            const SENT: u32 = 131;

            sleep(Duration::from_micros(100)).await;
            let a = select! {
                recv(&*ch1_clone) -> var => var.unwrap()
                recv(&ch2) -> var => var.unwrap()
                send(&ch3, RES) -> _var => SENT
            };

            assert_eq!(a, SENT, "blocking send assertion failed");
        });

        assert_eq!(
            ch1.recv().await.expect("failed to send"),
            RES,
            "blocking send assertion failed"
        );
    }

    // blocking recv err
    {
        let ch1 = LocalChannel::<u32>::bounded(1);
        let ch2 = LocalChannel::<u32>::bounded(1);
        let ch3 = LocalChannel::<u32>::bounded(0);

        local_scope(|scope| async {
            scope.spawn(async {
                sleep(Duration::from_micros(100)).await;
                ch2.close().await;
            });

            select! {
                recv(&ch1) -> _var => panic!("blocking recv with error failed")
                recv(&ch2) -> var => assert!(var.is_err(), "blocking recv with error failed")
                send(&ch3, 20) -> _var => panic!("blocking recv with error failed")
            }
        })
        .await;
    }

    // blocking send err
    {
        let ch1 = LocalChannel::<u32>::bounded(1);
        let ch2 = LocalChannel::<u32>::bounded(1);
        let ch3 = LocalChannel::<u32>::bounded(1);

        local_scope(|scope| async {
            scope.spawn(async {
                sleep(Duration::from_micros(100)).await;
                ch3.close().await;
            });

            select! {
                recv(&ch1) -> _var => panic!("blocking send with error failed")
                recv(&ch2) -> _var => panic!("blocking send with error failed")
                send(&ch3, 30) -> var => match var {
                    Err(SendErr::Closed(30)) => (),
                    _ => panic!("blocking send with error failed"),
                }
            }
        })
        .await;
    }
}
