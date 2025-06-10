use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
    task::{Poll, Waker},
    thread,
};

use log::trace;
use tokio::select;
use tokio_util::sync::CancellationToken;

use crate::{TError, TaskError, rt};

#[derive(Clone)]
pub struct Context<E: TError> {
    id: u32,
    inner: Arc<Mutex<ContextInner<E>>>,
    cancel: CancellationToken,
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

    pub fn is_active(&self) -> bool {
        !self.cancel.is_cancelled()
    }

    pub fn stop_with_result(&self, res: Option<TaskError<E>>) -> FutureTaskState<E> {
        let fur = self.wait_for(State::Stopped);
        let mut g = self.inner.lock().unwrap();
        if g.state >= State::Stopping {
            return fur;
        }
        let _ = g.set_state(State::Stopping);
        g.error = res;
        g.wake_all();
        drop(g);
        self.cancel.cancel();
        fur
    }

    pub fn spawn<F>(&self, fut: F)
    where
        F: Future + Send + 'static,
    {
        let mut g = self.inner.lock().unwrap();
        g.spawn(self, fut);
    }

    pub(crate) fn work_done(&self) {
        let mut g = self.inner.lock().unwrap();
        g.work_count -= 1;
        trace!("[{:>6}] work count {}", self.id, g.work_count);
        if g.work_count == 0 {
            let _ = g.set_state(State::Stopped);
        }
    }
}

impl<E: TError> Default for Context<E> {
    fn default() -> Self {
        static TASK_ID: AtomicU32 = AtomicU32::new(1);
        let id = TASK_ID.fetch_add(1, Ordering::SeqCst);

        Self {
            id,
            inner: Arc::new(Mutex::new(ContextInner {
                id,
                work_count: 1,
                ..Default::default()
            })),
            cancel: CancellationToken::new(),
        }
    }
}

struct ContextInner<E: TError> {
    error: Option<TaskError<E>>,
    state: State,
    wakers: Vec<Waker>,
    work_count: u32,
    id: u32,
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
        trace!("[{:>6}] [{:?}]=>[{:?}]", self.id, self.state, state);
        self.state = state;
        self.wake_all();
        Ok(())
    }

    fn spawn<F>(&mut self, ctx: &Context<E>, fur: F)
    where
        F: Future + Send + 'static,
    {
        let ctx = ctx.clone();
        if matches!(self.state, State::Stopping | State::Stopped) {
            return;
        }

        self.work_count += 1;
        trace!("[{:>6}] work count {}", ctx.id, self.work_count);
        thread::spawn(move || {
            rt().block_on(async move {
                select! {
                    _ = fur =>{}
                    _ = ctx.cancel.cancelled() => {}
                    _ = ctx.wait_for(State::Stopping) => {}
                }
                ctx.work_done();
            });
        });
    }
}

impl<E: TError> Default for ContextInner<E> {
    fn default() -> Self {
        Self {
            id: 0,
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
