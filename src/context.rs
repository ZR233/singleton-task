use std::{
    error::Error,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
    task::{Poll, Waker},
};

use crate::TaskError;

#[derive(Clone)]
pub struct Context<E: Error + Clone> {
    id: u32,
    inner: Arc<Mutex<ContextInner<E>>>,
}

impl<E: Error + Clone> Context<E> {
    pub fn id(&self) -> u32 {
        self.id
    }

    pub(crate) fn set_state(&self, state: State) -> Result<(), &'static str> {
        self.inner.lock().unwrap().set_state(state)
    }
}

impl<E: Error + Clone> Default for Context<E> {
    fn default() -> Self {
        static TASK_ID: AtomicU32 = AtomicU32::new(1);
        let id = TASK_ID.fetch_add(1, Ordering::SeqCst);

        Self {
            id,
            inner: Default::default(),
        }
    }
}

struct ContextInner<E: Error + Clone> {
    error: Option<E>,
    state: State,
    wakers: Vec<Waker>,
    work_count: u32,
}

impl<E: Error + Clone> ContextInner<E> {
    fn wake_all(&mut self) {
        for waker in self.wakers.iter() {
            waker.wake_by_ref();
        }
        self.wakers.clear();
    }

    fn set_state(&mut self, state: State) -> Result<(), &'static str> {
        if state < self.state {
            return Err("state is not allowed");
        }

        self.state = state;
        self.wake_all();
        Ok(())
    }
}

impl<E: Error + Clone> Default for ContextInner<E> {
    fn default() -> Self {
        Self {
            error: None,
            state: State::default(),
            wakers: Default::default(),
            work_count: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum State {
    Idle,
    Preparing,
    Running,
    Stopping,
    Stopped,
}

impl Default for State {
    fn default() -> Self {
        Self::Idle
    }
}

pub struct FutureTaskState<E: Error + Clone> {
    ctx: Context<E>,
    want: State,
}
impl<E: Error + Clone> FutureTaskState<E> {
    fn new(ctx: Context<E>, want: State) -> Self {
        Self { ctx, want }
    }
}

impl<E: Error + Clone> Future for FutureTaskState<E> {
    type Output = Result<(), TaskError<E>>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let mut g = self.ctx.inner.lock().unwrap();
        if g.state >= self.want {
            Poll::Ready(match g.error.clone() {
                Some(e) => Err(e.into()),
                None => Ok(()),
            })
        } else {
            g.wakers.push(cx.waker().clone());
            Poll::Pending
        }
    }
}
