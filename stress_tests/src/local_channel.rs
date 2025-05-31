use crate::acquire_global_lock;
use orengine::sync::{
    AsyncChannel, AsyncReceiver, AsyncSender, AsyncWaitGroup, LocalChannel, LocalWaitGroup,
    TryRecvErr, TrySendErr,
};
use orengine::{Local, local_executor, yield_now};

#[allow(clippy::future_not_send, reason = "Because it is test")]
async fn stress_test_local_channel_try(channel: LocalChannel<usize>) {
    const PAR: usize = 10;
    const COUNT: usize = 100;

    let guard = acquire_global_lock();

    for _ in 0..10 {
        let res = Local::new(0);
        let wg = LocalWaitGroup::new();

        wg.add(PAR * 2);
        for i in 0..PAR {
            local_executor().spawn_local(async {
                if i % 2 == 0 {
                    for j in 0..COUNT {
                        loop {
                            match channel.try_send(j) {
                                Ok(()) => break,
                                Err(e) => match e {
                                    TrySendErr::Full(_) | TrySendErr::Locked(_) => {
                                        yield_now().await;
                                    }
                                    TrySendErr::Closed(_) => panic!("send failed"),
                                },
                            }
                        }
                    }
                } else {
                    for j in 0..COUNT {
                        channel.send(j).await.unwrap();
                    }
                }

                wg.done();
            });

            local_executor().spawn_local(async {
                if i % 2 == 0 {
                    for _ in 0..COUNT {
                        loop {
                            match channel.try_recv() {
                                Ok(v) => {
                                    *res.borrow_mut() += v;
                                    break;
                                }
                                Err(e) => match e {
                                    TryRecvErr::Empty | TryRecvErr::Locked => {
                                        yield_now().await;
                                    }
                                    TryRecvErr::Closed => panic!("recv failed"),
                                },
                            }
                        }
                    }
                } else {
                    for _ in 0..COUNT {
                        let r = channel.recv().await.unwrap();
                        *res.borrow_mut() += r;
                    }
                }

                wg.done();
            });
        }

        wg.wait().await;

        assert_eq!(*res.borrow(), PAR * COUNT * (COUNT - 1) / 2);
    }

    drop(guard);
}

#[orengine::test_local(timeout_ms = 10000)]
fn stress_test_local_channel_try_unbounded() {
    stress_test_local_channel_try(LocalChannel::unbounded()).await;
}

#[orengine::test_local(timeout_ms = 10000)]
fn stress_test_local_channel_try_bounded() {
    stress_test_local_channel_try(LocalChannel::bounded(1024)).await;
}
