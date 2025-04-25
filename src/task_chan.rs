use std::sync::{Arc, Condvar, Mutex};

use crate::{Context, TError, WaitingTask};

pub fn task_channel<E: TError>() -> (TaskSender<E>, TaskReceiver<E>) {
    let inner = Arc::new(Inner {
        status: Mutex::new(Status {
            next: None,
            current: None,
            is_stopped: false,
        }),
        not_empty: Condvar::new(),
    });

    let sender = TaskSender {
        inner: Arc::clone(&inner),
    };

    let receiver = TaskReceiver { inner };

    (sender, receiver)
}

pub struct TaskSender<E: TError> {
    inner: Arc<Inner<E>>,
}

impl<E: TError> TaskSender<E> {
    pub fn send(&self, item: WaitingTask<E>) {
        let mut g = self.inner.status.lock().unwrap();
        if let Some(old) = g.next.replace(item) {
            old.task.ctx.stop();
        }
        if let Some(current) = &g.current {
            current.stop();
        }
        self.inner.not_empty.notify_one();
    }
}

impl<E: TError> Drop for TaskSender<E> {
    fn drop(&mut self) {
        let mut status = self.inner.status.lock().unwrap();
        status.is_stopped = true;
        self.inner.not_empty.notify_all();
    }
}

pub struct TaskReceiver<E: TError> {
    inner: Arc<Inner<E>>,
}

impl<E: TError> TaskReceiver<E> {
    pub fn recv(&self) -> Option<WaitingTask<E>> {
        let mut status = self.inner.status.lock().unwrap();
        loop {
            if status.is_stopped {
                return None;
            }
            if let Some(one) = status.next.take() {
                return Some(one);
            }
            status = self.inner.not_empty.wait(status).unwrap();
        }
    }
}

struct Inner<E: TError> {
    status: Mutex<Status<E>>,
    not_empty: Condvar,
}

struct Status<E: TError> {
    next: Option<WaitingTask<E>>,
    current: Option<Context<E>>,
    is_stopped: bool,
}
