use std::time::Duration;

pub(crate) struct SpawnManyTaskResult {
    pub(crate) task_count: usize,
    pub(crate) memory_usage_in_bytes: u64,
}

pub(crate) struct DurationResult {
    pub(crate) duration: Duration,
    pub(crate) number_of_repetitions: usize,
}

pub(crate) struct Results {
    pub(crate) create_small_task_and_yield: Option<DurationResult>,
    pub(crate) create_large_task_and_yield: Option<DurationResult>,
    pub(crate) lock_and_unlock_mutex: Option<DurationResult>,
    pub(crate) lock_for_read_and_unlock_rwlock: Option<DurationResult>,
    pub(crate) lock_for_write_and_read_and_unlock_rwlock: Option<DurationResult>,
    pub(crate) yield_task: Option<DurationResult>,
    pub(crate) spawn_many_tasks: Option<SpawnManyTaskResult>,
}

pub(crate) trait Runtime: Sized {
    const SMALL_TASK_SIZE: usize = 1;
    const LARGE_TASK_SIZE: usize = 9000;

    fn create_small_task_and_yield(&mut self) -> Option<DurationResult>;
    fn create_large_task_and_yield(&mut self) -> Option<DurationResult>;

    fn lock_and_update_and_unlock_mutex(&mut self) -> Option<DurationResult>;
    fn lock_for_read_and_unlock_rwlock(&mut self) -> Option<DurationResult>;
    fn lock_for_write_and_read_and_unlock_rwlock(&mut self) -> Option<DurationResult>;

    fn yield_task(&mut self) -> Option<DurationResult>;

    fn spawn_many_tasks(&mut self) -> Option<SpawnManyTaskResult>;

    fn bench() -> Results;
    fn name() -> &'static str;

    fn bench_and_print() {
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;
        const GB: u64 = MB * 1024;

        fn format_with_commas(n: u64) -> String {
            let s = n.to_string();
            let bytes = s.as_bytes();
            let mut result = Vec::with_capacity(s.len() + (s.len() - 1) / 3);
            let first_group_len = bytes.len() % 3;

            if first_group_len != 0 {
                for &b in &bytes[..first_group_len] {
                    result.push(b);
                }
                if bytes.len() > 3 {
                    result.push(b',');
                }
            }

            for (i, &b) in bytes[first_group_len..].iter().enumerate() {
                result.push(b);
                if (i + 1) % 3 == 0 && (i + 1) != bytes[first_group_len..].len() {
                    result.push(b',');
                }
            }
            String::from_utf8(result).unwrap()
        }

        fn print_duration(name: &str, duration_res: Option<DurationResult>) {
            if let Some(duration_res) = duration_res {
                let (number, dimension) = match duration_res.duration.as_nanos() {
                    ..1_000 => (duration_res.duration.as_nanos() as f64, "ns"),
                    1_000..1_000_000 => ((duration_res.duration.as_nanos() as f64) / 1_000.0, "us"),
                    1_000_000..1_000_000_000 => {
                        ((duration_res.duration.as_nanos() as f64) / 1_000_000.0, "ms")
                    }
                    _ => ((duration_res.duration.as_nanos() as f64) / 1_000_000_000.0, "s"),
                };

                // Calculate ops/s with better precision
                let ops_per_second =
                    duration_res.number_of_repetitions as f64 * 1_000_000_000.0
                        / duration_res.duration.as_nanos() as f64;

                // Calculate duration per op with better precision and choose appropriate unit
                let duration_per_op_nanos =
                    duration_res.duration.as_nanos() as f64 / duration_res.number_of_repetitions as f64;

                let (duration_per_op_value, duration_per_op_dimension) = match duration_per_op_nanos {
                    ..1_000.0 => (duration_per_op_nanos, "ns"),
                    1_000.0..1_000_000.0 => (duration_per_op_nanos / 1_000.0, "us"),
                    1_000_000.0..1_000_000_000.0 => (duration_per_op_nanos / 1_000_000.0, "ms"),
                    _ => (duration_per_op_nanos / 1_000_000_000.0, "s"),
                };
                let repetitions_str = format_with_commas(duration_res.number_of_repetitions as u64);
                let time_str = format!("{:.2}{}", number, dimension);
                let op = format!("{:.2}", duration_per_op_value);
                let op_time_str = format!("{:>6} {}/op", op, duration_per_op_dimension);
                let ops_per_sec_str = format_with_commas(ops_per_second as u64);

                println!(
                    "{:<50} {:<12} in {:<15} | {:<18} | {:<12} ops/s",
                    name,
                    repetitions_str,
                    time_str,
                    op_time_str,
                    ops_per_sec_str,
                );
            } else {
                println!("{}: N/A", name);
            }
        }

        let results = Self::bench();

        println!("\n{}:", Self::name());

        print_duration("create_small_task_and_yield", results.create_small_task_and_yield);
        print_duration("create_large_task_and_yield", results.create_large_task_and_yield);
        print_duration("lock_and_unlock_mutex", results.lock_and_unlock_mutex);
        print_duration("lock_for_read_and_unlock_rwlock", results.lock_for_read_and_unlock_rwlock);
        print_duration("lock_for_write_and_read_and_unlock_rwlock", results.lock_for_write_and_read_and_unlock_rwlock);
        print_duration("yield_task", results.yield_task);

        if let Some(spawn_many_tasks) = results.spawn_many_tasks {
            let (number, dimension) = match spawn_many_tasks.memory_usage_in_bytes {
                ..1024 => (spawn_many_tasks.memory_usage_in_bytes as f64, "B"),
                1024..MB => ((spawn_many_tasks.memory_usage_in_bytes as f64) / 1024.0, "KB"),
                MB..GB => ((spawn_many_tasks.memory_usage_in_bytes as f64) / MB as f64, "MB"),
                _ => ((spawn_many_tasks.memory_usage_in_bytes as f64) / GB as f64, "GB"),
            };

            let bytes_per_task =
                spawn_many_tasks.memory_usage_in_bytes as f64 / spawn_many_tasks.task_count as f64;

            // Calculate tasks/GB with better precision
            let tasks_per_gb =
                spawn_many_tasks.task_count as f64 * GB as f64 / spawn_many_tasks.memory_usage_in_bytes as f64;
            let task_count_str = format_with_commas(spawn_many_tasks.task_count as u64);
            let memory_str = format!("{:.2}{}", number, dimension);
            let bytes = format!("{:.2}", bytes_per_task);
            let bytes_per_task_str = format!("{:>6} bytes/task", bytes);
            let tasks_per_gb_str = format_with_commas(tasks_per_gb as u64);

            println!(
                "{:<50} {:<12} in {:<15} | {:<18} | {:<12} tasks/GB",
                "spawn_many_tasks", // Hardcoded name for alignment
                task_count_str,
                memory_str,
                bytes_per_task_str,
                tasks_per_gb_str
            );
        } else {
            println!("spawn_many_tasks: N/A");
        }
    }
}