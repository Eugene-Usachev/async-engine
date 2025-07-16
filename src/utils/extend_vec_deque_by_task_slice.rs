use crate::runtime::Task;
use std::collections::VecDeque;

/// This function extends the [`VecDeque`] by the [`slice`] of [`Task`].
///
/// It is based on [`VecDeque::extend`] which is optimized for slices.
pub(crate) fn extend_vec_deque_by_task_slice(vec_deque: &mut VecDeque<Task>, slice: &[Task]) {
    type CopiableTask = [u8; size_of::<Task>()];

    let stack_ref = unsafe { &*(slice as *const [Task] as *const [CopiableTask]) };
    let other_list_mut_ref = unsafe {
        &mut *std::ptr::from_mut::<VecDeque<Task>>(vec_deque)
            .cast::<VecDeque<CopiableTask>>()
    };

    other_list_mut_ref.extend(stack_ref);
}

#[cfg(test)]
mod tests {
    use crate as orengine;
    use crate::runtime::{Locality, Task};
    use crate::utils::extend_vec_deque_by_task_slice;
    use crate::{local_executor, Local};
    use std::collections::VecDeque;

    fn dummy_task(counter: Local<u32>, count: u32) -> Task {
        Task::allocate_new(async move {
            *counter.borrow_mut() += count;
        }, Locality::local())
    }


    #[orengine::test_local]
    fn test_extend_vec_deque_by_task_slice() {
        let mut vec_deque = VecDeque::new();
        let counter = Local::new(0);

        vec_deque.push_back(dummy_task(counter.clone(), 1));

        let tasks: Vec<Task> = (2..5).map(|i| dummy_task(counter.clone(), i)).collect();

        extend_vec_deque_by_task_slice(&mut vec_deque, &tasks);

        assert_eq!(vec_deque.len(), 4);

        for task in vec_deque.into_iter() {
            local_executor().exec_task_now(task);
        }

        assert_eq!(*counter.borrow(), 10);
    }

    #[orengine::test_local]
    fn test_extend_vec_deque_by_empty_task_slice() {
        let mut vec_deque = VecDeque::new();
        let counter = Local::new(0);

        vec_deque.push_back(dummy_task(counter.clone(), 1));
        vec_deque.push_back(dummy_task(counter.clone(), 2));

        extend_vec_deque_by_task_slice(&mut vec_deque, &[]);

        assert_eq!(vec_deque.len(), 2);

        for task in vec_deque.into_iter() {
            local_executor().exec_task_now(task);
        }

        assert_eq!(*counter.borrow(), 3);
    }
}