async fn example1() {
    use orengine::sync::{AsyncWaitGroup, LocalWaitGroup};
    use orengine::{local_executor, sleep, Local};
    use std::rc::Rc;
    use std::time::Duration;

    let wait_group = Rc::new(LocalWaitGroup::new()); // In `local` tasks we can use Rc and local primitives of synchronization
    let number_executed_tasks = Local::new(0); // We can use `Local` in `local` tasks

    for i in 0..10 {
        let wait_group = wait_group.clone();
        let number_executed_tasks = number_executed_tasks.clone();

        wait_group.inc();
        local_executor().spawn_local(async move {
            // `local` task is spawned in the current thread and will not leave it for the rest of its life.
            sleep(Duration::from_millis(i)).await;

            *number_executed_tasks.borrow_mut() += 1;

            wait_group.done();
        });
    }

    wait_group.wait().await; // wait until all tasks are completed
    assert_eq!(*number_executed_tasks.borrow(), 10);
}

async fn example2() {
    use orengine::sync::{AsyncWaitGroup, WaitGroup};
    use orengine::{local_executor, sleep};
    use std::sync::{
        atomic::{AtomicUsize, Ordering::Relaxed},
        Arc,
    };
    use std::time::Duration;

    let wait_group = Arc::new(WaitGroup::new()); // In `shared` tasks we can't use Rc nor local primitives of synchronization
    let number_executed_tasks = Arc::new(AtomicUsize::new(0)); // We should use the primitive of synchronization

    for i in 0..10 {
        let wait_group = wait_group.clone();
        let number_executed_tasks = number_executed_tasks.clone();

        wait_group.inc();
        local_executor().spawn_shared(async move {
            // `shared` task is spawned in the current thread, but can leave it.
            sleep(Duration::from_millis(i)).await;

            number_executed_tasks.fetch_add(1, Relaxed);

            wait_group.done();
        });
    }

    wait_group.wait().await; // wait until all tasks are completed
    assert_eq!(number_executed_tasks.load(Relaxed), 10);
}
