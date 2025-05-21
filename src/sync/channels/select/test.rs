use crate as orengine;
use crate::sync::{AsyncChannel, AsyncReceiver, AsyncSender, Channel, LocalChannel, SendErr};
use crate::{local_executor, sleep};
use orengine::select;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

// region `local` not stress tests

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

        ch2.try_send(31).expect("failed to send");

        let a = select! {
            recv(&ch1) -> var => var.unwrap(),
            recv(&ch2) -> var => var.unwrap(),
            send(&ch2, 20) -> _var => 1,
            default => 4
        };

        assert_eq!(a, RES, "non-blocking recv assertion failed");
    }

    // non-blocking `send` success
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
