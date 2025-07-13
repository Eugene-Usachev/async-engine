use orengine::sync::{
    AsyncChannel, AsyncReceiver, AsyncSender, AsyncWaitGroup, Channel, TryRecvErr, TrySendErr,
    WaitGroup,
};
use orengine::test::sched_future;
use orengine::utils::get_core_ids;
use orengine::{local_executor, yield_now};
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::{Relaxed, SeqCst};

async fn stress_test(channel: Channel<usize>, count: usize) {
    let channel = Arc::new(channel);
    for _ in 0..20 {
        let wg = Arc::new(WaitGroup::new());
        let sent = Arc::new(AtomicUsize::new(0));
        let received = Arc::new(AtomicUsize::new(0));

        for i in 0..get_core_ids().unwrap().len() * 2 {
            let channel = channel.clone();
            let wg = wg.clone();
            let sent = sent.clone();
            let received = received.clone();

            wg.add(1).await;

            sched_future(async move {
                if i % 2 == 0 {
                    for j in 0..count {
                        channel.send(j).await.unwrap();
                        sent.fetch_add(j, Relaxed);
                    }
                } else {
                    for _ in 0..count {
                        let res = channel.recv().await.unwrap();
                        received.fetch_add(res, Relaxed);
                    }
                }

                wg.done().await;
            });
        }

        wg.wait().await;

        assert_eq!(sent.load(Relaxed), received.load(Relaxed));
    }
}

#[orengine::test::test_shared(timeout_ms = 10000, exclusive_in = "*")]
fn stress_test_bounded_shared_channel() {
    stress_test(Channel::bounded(1024), 1000).await;
}

#[orengine::test::test_shared(timeout_ms = 10000, exclusive_in = "*")]
fn stress_test_unbounded_shared_channel() {
    stress_test(Channel::unbounded(), 1000).await;
}

#[orengine::test::test_shared(timeout_ms = 10000, exclusive_in = "*")]
fn stress_test_zero_capacity_shared_channel() {
    stress_test(Channel::bounded(0), 200).await;
}

#[allow(clippy::future_not_send, reason = "Because it is test")]
async fn stress_test_local_channel_try(original_channel: Arc<Channel<usize>>) {
    const PAR: usize = 10;
    const COUNT: usize = 1000;

    for _ in 0..100 {
        let original_res = Arc::new(AtomicUsize::new(0));
        let original_wg = Arc::new(WaitGroup::new());

        for i in 0..PAR {
            let wg = original_wg.clone();
            let channel = original_channel.clone();

            wg.inc().await;

            local_executor().spawn_shared(async move {
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

            let wg = original_wg.clone();
            let res = original_res.clone();
            let channel = original_channel.clone();

            wg.inc().await;

            local_executor().spawn_shared(async move {
                if i % 2 == 0 {
                    for _ in 0..COUNT {
                        loop {
                            match channel.try_recv() {
                                Ok(v) => {
                                    res.fetch_add(v, SeqCst);
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
                        res.fetch_add(r, SeqCst);
                    }
                }

                wg.done().await;
            });
        }

        original_wg.wait().await;

        assert_eq!(original_res.load(SeqCst), PAR * COUNT * (COUNT - 1) / 2);
    }
}

#[orengine::test::test_shared(timeout_ms = 10000, exclusive_in = "*")]
fn stress_test_local_channel_try_unbounded() {
    stress_test_local_channel_try(Arc::new(Channel::unbounded())).await;
}

#[orengine::test::test_shared(timeout_ms = 10000, exclusive_in = "*")]
fn stress_test_local_channel_try_bounded() {
    stress_test_local_channel_try(Arc::new(Channel::bounded(1024))).await;
}
