use orengine::sync::{AsyncWaitGroup, WaitGroup};
use orengine::{Executor, local_executor, yield_now};
use std::ops::Add;
#[cfg(test)]
use std::thread;
use std::time::Duration;

#[cfg(test)]
mod local_channel;
#[cfg(test)]
mod mutex;
#[cfg(test)]
mod rw_lock;
#[cfg(test)]
mod select;
#[cfg(test)]
mod shared_channel;

#[cfg(test)]
pub fn sched_future<F: Future<Output = ()> + 'static + Send>(future: F) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        Executor::init().run_with_shared_future(future);
    })
}

#[cfg(test)]
pub async fn sync_wait_wg(wg: &WaitGroup) {
    let mut time_to_sleep = Duration::from_millis(1);

    while wg.count().await > 0 {
        if local_executor().number_of_spawned_tasks() == 0 {
            thread::sleep(time_to_sleep);
        }

        yield_now().await;

        time_to_sleep = Duration::from_millis(5)
            .add(time_to_sleep)
            .min(Duration::from_millis(50));
    }
}
