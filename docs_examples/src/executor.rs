use std::io::Write;

fn example1() {
    use orengine::{local_executor, Executor};

    let executor = Executor::init(); // initializes the executor in the current thread and returns 'static reference to it

    assert_eq!(executor.id(), local_executor().id());
}

fn example2() {
    use orengine::runtime::Config;
    use orengine::utils::get_core_ids;
    use orengine::{local_executor, Executor};

    let config_for_executor_with_only_cpu_bounded_tasks = Config::default()
        .set_buffer_cap(8192) // sets the capacity of `Buffer`
        .set_work_sharing_level(2) // very aggressive `work-sharing`, almost `work-stealing`
        .set_numbers_of_blocking_workers(0) // disables the thread pool
        .disable_io_worker(); // disables the IO worker

    let core_ids = get_core_ids().unwrap();

    if core_ids.len() > 1 {
        let id = core_ids[0];

        std::thread::spawn(move || {
            Executor::init_on_core(id); // thread will be bound to this code and `Executor` will be created with default config
        });
    }

    let executor = Executor::init_on_core_with_config(
        core_ids[0], // thread will be bound to this core
        config_for_executor_with_only_cpu_bounded_tasks,
    ); // initializes the executor in the current thread and returns 'static reference to it

    assert_eq!(executor.id(), local_executor().id());
}

fn example3() {
    orengine::Executor::init().run(); // run `Executor` forever. Thread will be blocked until `Executor` is stopped.

    println!("Executor was stopped");
}

fn example4() {
    use orengine::{runtime, Executor};

    let ex = Executor::init();
    let id = ex.id();

    ex.run_with_local_future(async {
        println!("Hello from an async runtime!");

        runtime::stop_executor(id);
    });

    println!("Executor was stopped");
}

fn example5() {
    use orengine::Executor;

    let ex = Executor::init();

    let answer = ex
        .run_and_block_on_local(async {
            println!("Hello from an async runtime!");

            42
        })
        .expect("UB is happened");

    assert_eq!(answer, 42);
}

fn example6() {
    use orengine::local_executor;

    print!("1 ");

    local_executor().exec_local_future(async {
        print!("2 ");
    });

    print!("3");

    std::io::stdout().flush().unwrap();

    // Likely will print "1 2 3"
}
