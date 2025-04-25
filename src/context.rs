use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
    task::{Poll, Waker},
};

use log::trace;

use crate::{TError, TaskError};

#[derive(Clone)]
pub struct Context<E: TError> {
    id: u32,
    inner: Arc<Mutex<ContextInner<E>>>,
}

impl<E: TError> Context<E> {
    pub fn id(&self) -> u32 {
        self.id
    }

    pub(crate) fn set_state(&self, state: State) -> Result<(), &'static str> {
        self.inner.lock().unwrap().set_state(state)
    }

    pub fn wait_for(&self, state: State) -> FutureTaskState<E> {
        FutureTaskState::new(self.clone(), state)
    }

    pub fn stop(&self) -> FutureTaskState<E> {
        self.stop_with_result(Some(TaskError::Cancelled))
    }

    pub fn stop_with_result(&self, res: Option<TaskError<E>>) -> FutureTaskState<E> {
        let fur = self.wait_for(State::Stopped);
        let mut g = self.inner.lock().unwrap();
        if g.state >= State::Stopping {
            return fur;
        }
        trace!("cancel all");
        let _ = g.set_state(State::Stopping);
        g.error = res;
        g.wake_all();
        fur
    }
}

impl<E: TError> Default for Context<E> {
    fn default() -> Self {
        static TASK_ID: AtomicU32 = AtomicU32::new(1);
        let id = TASK_ID.fetch_add(1, Ordering::SeqCst);

        Self {
            id,
            inner: Default::default(),
        }
    }
}

struct ContextInner<E: TError> {
    error: Option<TaskError<E>>,
    state: State,
    wakers: Vec<Waker>,
    work_count: u32,
}

impl<E: TError> ContextInner<E> {
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
        trace!("[{:?}]=>[{:?}]", self.state, state);
        self.state = state;
        self.wake_all();
        Ok(())
    }
}

impl<E: TError> Default for ContextInner<E> {
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

pub struct FutureTaskState<E: TError> {
    ctx: Context<E>,
    want: State,
}
impl<E: TError> FutureTaskState<E> {
    fn new(ctx: Context<E>, want: State) -> Self {
        Self { ctx, want }
    }
}

impl<E: TError> Future for FutureTaskState<E> {
    type Output = Result<(), TaskError<E>>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let mut g = self.ctx.inner.lock().unwrap();
        if g.state >= self.want {
            Poll::Ready(match g.error.clone() {
                Some(e) => Err(e),
                None => Ok(()),
            })
        } else {
            g.wakers.push(cx.waker().clone());
            Poll::Pending
        }
    }
}
