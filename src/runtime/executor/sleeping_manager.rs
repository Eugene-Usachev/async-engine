use crate::local_executor;
use crate::runtime::{Task, TaskWithDeadline};
use crate::sync::channels::waiting_task::TaskInSelectBranch;
use crate::utils::OrengineInstant;
use std::cmp::min;
use std::collections::BTreeMap;
use std::collections::btree_map::Entry::{Occupied, Vacant};
use std::time::Duration;

/// Manages the tasks that are currently sleeping.
pub(super) struct SleepingManager {
    /// The tasks that are currently [`sleeping`](crate::sleep).
    sleeping_tasks: BTreeMap<OrengineInstant, Task>,
    /// The tasks that are currently waiting for a deadline or a recv/send operation.
    tasks_with_deadline: BTreeMap<OrengineInstant, TaskWithDeadline>,
    /// The tasks in `select` that are currently waiting for a deadline.
    tasks_in_select_with_deadline: BTreeMap<OrengineInstant, TaskInSelectBranch>,
}

impl SleepingManager {
    /// Creates a new [`SleepingManager`].
    pub(super) const fn new() -> Self {
        Self {
            sleeping_tasks: BTreeMap::new(),
            tasks_with_deadline: BTreeMap::new(),
            tasks_in_select_with_deadline: BTreeMap::new(),
        }
    }

    /// Inserts a task with a deadline into the map.
    ///
    /// It can increase the deadline if there is already a task with the same deadline.
    fn insert_deadline<T>(
        map: &mut BTreeMap<OrengineInstant, T>,
        mut deadline: OrengineInstant,
        task: T,
    ) {
        loop {
            match map.entry(deadline) {
                Vacant(entry) => {
                    entry.insert(task);

                    break;
                }
                Occupied(_) => {
                    deadline += Duration::from_nanos(1);
                }
            }
        }
    }

    /// Registers a [`task`](Task) to be executed at the provided [`OrengineInstant`].
    ///
    /// It can increase the deadline if there is already a task with the same deadline.
    pub(super) fn register_sleeping_task(&mut self, deadline: OrengineInstant, task: Task) {
        Self::insert_deadline(&mut self.sleeping_tasks, deadline, task);
    }

    /// Registers a [`TaskWithDeadline`] to be executed at the provided [`OrengineInstant`].
    ///
    /// It can increase the deadline if there is already a task with the same deadline.
    pub(super) fn register_task_with_deadline(
        &mut self,
        deadline: OrengineInstant,
        task: TaskWithDeadline,
    ) {
        Self::insert_deadline(&mut self.tasks_with_deadline, deadline, task);
    }

    /// Registers a [`TaskInSelectBranch`] to be executed at the provided [`OrengineInstant`].
    ///
    /// It can increase the deadline if there is already a task with the same deadline.
    pub(super) fn register_task_in_select_with_deadline(
        &mut self,
        deadline: OrengineInstant,
        task: TaskInSelectBranch,
    ) {
        Self::insert_deadline(&mut self.tasks_in_select_with_deadline, deadline, task);
    }

    /// Polls the map for tasks that are ready to be executed.
    ///
    /// It removes the tasks from the map and calls the provided function with the task.
    ///
    /// It returns the nearest deadline.
    fn poll_map<T>(
        map: &mut BTreeMap<OrengineInstant, T>,
        now: OrengineInstant,
        mut fn_to_wake: impl FnMut(T),
    ) -> Option<OrengineInstant> {
        while let Some((deadline, task)) = map.pop_first() {
            if deadline > now {
                map.insert(deadline, task);

                return Some(deadline);
            }

            fn_to_wake(task);
        }

        None
    }

    /// Polls the map for tasks that are ready to be executed.
    ///
    /// It removes the tasks from the map and calls the provided function with the task.
    ///
    /// It returns the nearest deadline.
    pub(super) fn poll(&mut self, now: OrengineInstant) -> Option<OrengineInstant> {
        fn wake_task(task: Task) {
            match (task.is_local(), cfg!(test)) {
                (true, _) => local_executor().exec_task(task),
                (false, true) => local_executor().spawn_task_at_end_of_shared_tasks_queue(task),
                (false, false) => local_executor().spawn_shared_task(task),
            }
        }

        let mut the_nearest_deadline;

        the_nearest_deadline = Self::poll_map(&mut self.sleeping_tasks, now, |task| {
            wake_task(task);
        });

        the_nearest_deadline = Self::poll_map(&mut self.tasks_with_deadline, now, |task| {
            task.try_wake_by_deadline();
        })
        .map_or(the_nearest_deadline, |deadline| {
            the_nearest_deadline.map_or(Some(deadline), |prev_deadline| {
                Some(min(prev_deadline, deadline))
            })
        });

        Self::poll_map(
            &mut self.tasks_in_select_with_deadline,
            now,
            |task_in_select_branch| {
                if let Some(task) = task_in_select_branch.acquire_once() {
                    wake_task(task);
                }
            },
        )
        .map_or(the_nearest_deadline, |deadline| {
            the_nearest_deadline.map_or(Some(deadline), |prev_deadline| {
                Some(min(prev_deadline, deadline))
            })
        })
    }
}
