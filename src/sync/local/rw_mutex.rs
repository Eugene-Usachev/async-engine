use std::future::Future;
use std::intrinsics::{likely, unlikely};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::task::{Context, Poll};
use crate::Executor;
use crate::local::Local;
use crate::runtime::task::Task;

pub struct LocalReadMutexGuard<T> {
    local_rw_mutex: LocalRWMutex<T>
}

impl<T> LocalReadMutexGuard<T> {
    #[inline(always)]
    fn new(local_rw_mutex: LocalRWMutex<T>) -> Self {
        Self { local_rw_mutex }
    }
}

impl<T> Deref for LocalReadMutexGuard<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.local_rw_mutex.inner.get().value
    }
}

impl<T> Drop for LocalReadMutexGuard<T> {
    fn drop(&mut self) {
        self.local_rw_mutex.drop_read();
    }
}

pub struct LocalWriteMutexGuard<T> {
    local_rw_mutex: LocalRWMutex<T>
}

impl<T> LocalWriteMutexGuard<T> {
    #[inline(always)]
    fn new(local_rw_mutex: LocalRWMutex<T>) -> Self {
        Self { local_rw_mutex }
    }
}

impl<T> Deref for LocalWriteMutexGuard<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.local_rw_mutex.inner.get().value
    }
}

impl<T> DerefMut for LocalWriteMutexGuard<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.local_rw_mutex.inner.get_mut().value
    }
}

impl<T> Drop for LocalWriteMutexGuard<T> {
    fn drop(&mut self) {
        self.local_rw_mutex.drop_write();
    }
}

pub struct ReadMutexWait<T> {
    need_wait: bool,
    local_rw_mutex: LocalRWMutex<T>,
    phantom_data: PhantomData<T>
}

impl<T> ReadMutexWait<T> {
    #[inline(always)]
    fn new(need_wait: bool, local_rw_mutex: LocalRWMutex<T>) -> Self {
        Self {
            need_wait,
            local_rw_mutex,
            phantom_data: PhantomData
        }
    }
}

impl<T> Future for ReadMutexWait<T> {
    type Output = LocalReadMutexGuard<T>;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        if unlikely(this.need_wait) {
            let task = unsafe { (_cx.waker().as_raw().data() as *const Task).read() };
            this.local_rw_mutex.inner.get_mut().wait_queue_read.push(task);
            this.need_wait = false;
            Poll::Pending
        } else {
            Poll::Ready(LocalReadMutexGuard::new(this.local_rw_mutex.clone()))
        }
    }
}

pub struct WriteMutexWait<T> {
    need_wait: bool,
    local_rw_mutex: LocalRWMutex<T>,
    phantom_data: PhantomData<T>
}

impl<T> WriteMutexWait<T> {
    #[inline(always)]
    fn new(need_wait: bool, local_rw_mutex: LocalRWMutex<T>) -> Self {
        Self {
            need_wait,
            local_rw_mutex,
            phantom_data: PhantomData
        }
    }
}

impl<T> Future for WriteMutexWait<T> {
    type Output = LocalWriteMutexGuard<T>;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        if unlikely(this.need_wait) {
            let task = unsafe { (_cx.waker().as_raw().data() as *const Task).read() };
            this.local_rw_mutex.inner.get_mut().wait_queue_write.push(task);
            this.need_wait = false;
            Poll::Pending
        } else {
            Poll::Ready(LocalWriteMutexGuard::new(this.local_rw_mutex.clone()))
        }
    }
}

struct Inner<T> {
    wait_queue_read: Vec<Task>,
    wait_queue_write: Vec<Task>,
    number_of_readers: isize,
    value: T
}

pub struct LocalRWMutex<T> {
    inner: Local<Inner<T>>
}

impl<T> LocalRWMutex<T> {
    #[inline(always)]
    pub fn new(value: T) -> LocalRWMutex<T> {
        LocalRWMutex {
            inner: Local::new(Inner {
                wait_queue_read: Vec::new(),
                wait_queue_write: Vec::new(),
                number_of_readers: 0,
                value
            })
        }
    }

    #[inline(always)]
    pub fn write(&self) -> WriteMutexWait<T> {
        let inner = self.inner.get_mut();

        if unlikely(inner.number_of_readers == 0) {
            debug_assert!(inner.wait_queue_read.is_empty());

            inner.number_of_readers = -1;
            return WriteMutexWait::new(false, self.clone());
        }

        WriteMutexWait::new(true, self.clone())
    }

    #[inline(always)]
    pub fn read(&self) -> ReadMutexWait<T> {
        let inner = self.inner.get_mut();

        if likely(inner.number_of_readers > -1) {
            inner.number_of_readers += 1;
            ReadMutexWait::new(false, self.clone())
        } else {
            ReadMutexWait::new(true, self.clone())
        }
    }

    #[inline(always)]
    pub fn try_write(&self) -> Option<LocalWriteMutexGuard<T>> {
        let inner = self.inner.get_mut();

        if unlikely(inner.number_of_readers == 0) {
            debug_assert!(inner.wait_queue_read.is_empty());

            inner.number_of_readers = -1;
            Some(LocalWriteMutexGuard::new(self.clone()))
        } else {
            None
        }
    }

    #[inline(always)]
    pub fn try_read(&self) -> Option<LocalReadMutexGuard<T>> {
        let inner = self.inner.get_mut();
        if likely(inner.number_of_readers > -1) {
            inner.number_of_readers += 1;
            Some(LocalReadMutexGuard::new(self.clone()))
        } else {
            None
        }
    }

    #[inline(always)]
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.inner.get_mut().value
    }

    #[inline(always)]
    fn drop_read(&self) {
        let inner = self.inner.get_mut();
        inner.number_of_readers -= 1;

        if inner.number_of_readers == 0 {
            debug_assert!(inner.wait_queue_read.is_empty());
            let task = inner.wait_queue_write.pop();
            if unlikely(task.is_some()) {
                inner.number_of_readers = -1;
                Executor::exec_task(unsafe { task.unwrap_unchecked() });
            }
        }
    }

    #[inline(always)]
    fn drop_write(&self) {
        let inner = self.inner.get_mut();

        let task = inner.wait_queue_write.pop();
        if unlikely(task.is_some()) {
            Executor::exec_task(task.unwrap());
        } else {
            let mut readers_count = inner.wait_queue_read.len();
            inner.number_of_readers = readers_count as isize;
            while readers_count > 0 {
                let task = inner.wait_queue_read.pop();
                Executor::exec_task(unsafe { task.unwrap_unchecked() });
                readers_count -= 1;
            }
        }
    }
}

impl<T> Clone for LocalRWMutex<T> {
    fn clone(&self) -> Self {
        LocalRWMutex {
            inner: self.inner.clone()
        }
    }
}

unsafe impl<T> Sync for LocalRWMutex<T> {}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};
    use crate::runtime::{create_local_executer_for_block_on, local_executor};
    use crate::sleep::sleep;
    use crate::sync::LocalWaitGroup;
    use super::*;

    #[test]
    fn test_rw_mutex() {
        const SLEEP_DURATION: Duration = Duration::from_millis(1);

        create_local_executer_for_block_on(async {
            let start = Instant::now();
            let mutex = LocalRWMutex::new(0);
            let wg = LocalWaitGroup::new();
            let read_wg = LocalWaitGroup::new();

            for i in 1..=100 {
                let mutex = mutex.clone();
                local_executor().exec_future(async move {
                    let value = mutex.read().await;
                    assert_eq!(mutex.inner.get().number_of_readers, i);
                    assert_eq!(*value, 0);
                    sleep(SLEEP_DURATION).await;
                    assert_eq!(mutex.inner.get().number_of_readers, 101 - i);
                    assert_eq!(*value, 0);
                });
            }

            for _ in 1..=100 {
                let wg = wg.clone();
                let read_wg = read_wg.clone();
                wg.add(1);
                let mutex = mutex.clone();
                local_executor().exec_future(async move {
                    assert_eq!(mutex.inner.get().number_of_readers, 100);
                    let mut value = mutex.write().await;
                    {
                        let read_wg = read_wg.clone();
                        let mutex = mutex.clone();
                        read_wg.add(1);

                        local_executor().exec_future(async move {
                            assert_eq!(mutex.inner.get().number_of_readers, -1);
                            let value = mutex.read().await;
                            assert_ne!(*value, 0);
                            assert_ne!(mutex.inner.get().number_of_readers, 0);
                            read_wg.done();
                        });
                    }
                    let elapsed = start.elapsed();
                    assert!(elapsed >= SLEEP_DURATION);
                    assert_eq!(mutex.inner.get().number_of_readers, -1);
                    *value += 1;

                    wg.done();
                });
            }

            wg.wait().await;
            read_wg.wait().await;

            let value = mutex.read().await;
            assert_eq!(*value, 100);
            assert_ne!(mutex.inner.get().number_of_readers, 0);
        });
    }

    #[test]
    fn test_try_rw_mutex() {
        const SLEEP_DURATION: Duration = Duration::from_millis(1);

        create_local_executer_for_block_on(async {
            let start = Instant::now();
            let mutex = LocalRWMutex::new(0);
            let wg = LocalWaitGroup::new();
            let read_wg = LocalWaitGroup::new();

            for i in 1..=100 {
                let mutex = mutex.clone();
                local_executor().exec_future(async move {
                    let value = mutex.try_read().expect("try_read failed");
                    assert_eq!(mutex.inner.get().number_of_readers, i);
                    assert_eq!(*value, 0);
                    sleep(SLEEP_DURATION).await;
                });
            }

            for _i in 1..=100 {
                let wg = wg.clone();
                let read_wg = read_wg.clone();
                wg.add(1);
                let mutex = mutex.clone();
                local_executor().exec_future(async move {
                    assert_eq!(mutex.inner.get().number_of_readers, 100);
                    assert!(mutex.try_write().is_none());
                    sleep(2 * SLEEP_DURATION).await;
                    let mut value = mutex.try_write().expect("try_write failed");
                    read_wg.add(1);
                    {
                        let mutex = mutex.clone();

                        local_executor().exec_future(async move {
                            assert_eq!(mutex.inner.get().number_of_readers, -1);
                            assert!(mutex.try_read().is_none());
                            sleep(SLEEP_DURATION * 2).await;
                            let value = mutex.try_read().expect("try_read failed");
                            assert_ne!(*value, 0);
                            assert_ne!(mutex.inner.get().number_of_readers, 0);
                            read_wg.done();
                        });
                    }
                    let elapsed = start.elapsed();
                    assert!(elapsed >= SLEEP_DURATION);
                    assert_eq!(mutex.inner.get().number_of_readers, -1);
                    *value += 1;

                    wg.done();
                });
            }

            wg.wait().await;
            read_wg.wait().await;

            let value = mutex.try_read().expect("try_read failed");
            assert_eq!(*value, 100);
            assert_ne!(mutex.inner.get().number_of_readers, 0);
        });
    }
}