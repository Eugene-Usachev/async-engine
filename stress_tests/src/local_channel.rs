use orengine::sync::{
    AsyncChannel, AsyncReceiver, AsyncSender, AsyncWaitGroup, LocalChannel, LocalWaitGroup,
    TryRecvErr, TrySendErr,
};
use orengine::{Local, local_executor, yield_now};
use std::rc::Rc;

#[allow(clippy::future_not_send, reason = "Because it is test")]
async fn stress_test_local_channel_try(channel: LocalChannel<usize>) {
    const PAR: usize = 10;
    const COUNT: usize = 100;

    let channel = Rc::new(channel);

    for _ in 0..10 {
        let res = Local::new(0);
        let wg = Rc::new(LocalWaitGroup::new());

        wg.add(PAR * 2).await;

        for i in 0..PAR {
            let wg = wg.clone();
            let wg2 = wg.clone();
            let channel = channel.clone();
            let channel2 = channel.clone();
            let res = res.clone();
            let res2 = res.clone();

            local_executor().spawn_local(async move {
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

                wg.done().await;
            });

            local_executor().spawn_local(async move {
                if i % 2 == 0 {
                    for _ in 0..COUNT {
                        loop {
                            match channel2.try_recv() {
                                Ok(v) => {
                                    *res2.borrow_mut() += v;
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
                        let r = channel2.recv().await.unwrap();
                        *res2.borrow_mut() += r;
                    }
                }

                wg2.done().await;
            });
        }

        wg.wait().await;

        assert_eq!(*res.borrow(), PAR * COUNT * (COUNT - 1) / 2);
    }
}

#[orengine::test_local(timeout_ms = 10000)]
fn stress_test_local_channel_try_unbounded() {
    stress_test_local_channel_try(LocalChannel::unbounded()).await;
}

#[orengine::test_local(timeout_ms = 10000)]
fn stress_test_local_channel_try_bounded() {
    stress_test_local_channel_try(LocalChannel::bounded(1024)).await;
}
