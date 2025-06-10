use std::{
    sync::{Arc, Mutex},
    task::{Poll, Waker},
};

use crate::{Context, TError, WaitingTask, context::State};

pub fn task_channel<E: TError>() -> (TaskSender<E>, TaskReceiver<E>) {
    let inner = Arc::new(Inner {
        status: Mutex::new(Status {
            next: None,
            current: None,
            is_stopped: false,
            wakers: vec![],
        }),
    });

    let sender = TaskSender {
        inner: Arc::clone(&inner),
    };

    let receiver = TaskReceiver { inner };

    (sender, receiver)
}

#[derive(Clone)]
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
        for w in g.wakers.iter() {
            w.wake_by_ref();
        }
        g.wakers.clear();
    }

    pub fn stop(&self) {
        let mut status = self.inner.status.lock().unwrap();
        status.is_stopped = true;

        let mut g = self.inner.status.lock().unwrap();
        for w in g.wakers.iter() {
            w.wake_by_ref();
        }
        g.wakers.clear();
    }
}

pub struct TaskReceiver<E: TError> {
    inner: Arc<Inner<E>>,
}

impl<E: TError> TaskReceiver<E> {
    pub fn recv(&self) -> impl Future<Output = Option<WaitingTask<E>>> {
        WaitRcv {
            inner: self.inner.clone(),
        }
    }
}

struct WaitRcv<E: TError> {
    inner: Arc<Inner<E>>,
}

impl<E: TError> Future for WaitRcv<E> {
    type Output = Option<WaitingTask<E>>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let mut status = self.inner.status.lock().unwrap();
        if status.is_stopped {
            return Poll::Ready(None);
        }
        if let Some(one) = status.next.take() {
            status.current = Some(one.task.ctx.clone());
            let _ = one.task.ctx.set_state(State::Preparing);
            return Poll::Ready(Some(one));
        }
        status.wakers.push(cx.waker().clone());
        Poll::Pending
    }
}

struct Inner<E: TError> {
    status: Mutex<Status<E>>,
}

struct Status<E: TError> {
    next: Option<WaitingTask<E>>,
    current: Option<Context<E>>,
    is_stopped: bool,
    wakers: Vec<Waker>,
}
