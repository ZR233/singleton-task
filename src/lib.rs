use std::{error::Error, fmt::Display};

pub use async_trait::async_trait;
pub use tokio::{
    sync::mpsc::{Receiver, Sender},
    task::JoinHandle,
};

use log::{trace, warn};
use tokio::{select, sync::mpsc::channel};

mod context;
mod task_chan;

pub use context::Context;

use context::{FutureTaskState, State};
use task_chan::{TaskReceiver, TaskSender, task_channel};

pub trait TError: Error + Clone + Send + 'static {}

#[derive(Debug, Clone)]
pub enum TaskError<E: TError> {
    Cancelled,
    Error(E),
}

impl<E: TError> From<E> for TaskError<E> {
    fn from(value: E) -> Self {
        Self::Error(value)
    }
}

impl<E: TError> Display for TaskError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => write!(f, "Cancelled"),
            Self::Error(e) => write!(f, "{e}"),
        }
    }
}

pub trait TaskBuilder {
    type Output: Send + 'static;
    type Error: TError;
    type Task: Task<Self::Error>;

    fn build(self, tx: Sender<Self::Output>) -> Self::Task;
    fn channel_size(&self) -> usize {
        10
    }
}

#[async_trait]
pub trait Task<E: TError>: Send + 'static {
    async fn on_start(&mut self, ctx: Context<E>) -> Result<(), E> {
        drop(ctx);
        trace!("on_start");
        Ok(())
    }
    async fn on_stop(&mut self, ctx: Context<E>) -> Result<(), E> {
        drop(ctx);
        trace!("on_stop");
        Ok(())
    }
}

struct TaskBox<E: TError> {
    task: Box<dyn Task<E>>,
    ctx: Context<E>,
}

struct WaitingTask<E: TError> {
    task: TaskBox<E>,
}

#[derive(Clone)]
pub struct SingletonTask<E: TError> {
    tx: TaskSender<E>,
}

impl<E: TError> SingletonTask<E> {
    pub fn new() -> Self {
        let (tx, rx) = task_channel::<E>();

        tokio::spawn(Self::work_deal_start(rx));

        Self { tx }
    }

    async fn work_deal_start(rx: TaskReceiver<E>) {
        while let Some(next) = rx.recv().await {
            let id = next.task.ctx.id();
            if let Err(e) = Self::work_start_task(next).await {
                warn!("task [{id}] error: {e}");
            }
        }
        trace!("task work done");
    }

    async fn work_start_task(next: WaitingTask<E>) -> Result<(), TaskError<E>> {
        trace!("run task {}", next.task.ctx.id());
        let ctx = next.task.ctx.clone();
        let mut task = next.task.task;
        match select! {
            res = task.on_start(ctx.clone()) => res.map_err(|e|e.into()),
            res = ctx.wait_for(State::Stopping) => res
        } {
            Ok(_) => {
                if ctx.set_state(State::Running).is_err() {
                    return Err(TaskError::Cancelled);
                };
            }
            Err(e) => {
                ctx.stop_with_terr(e);
            }
        }

        let _ = ctx.wait_for(State::Stopping).await;
        let _ = task.on_stop(ctx.clone()).await;
        ctx.work_done();
        let _ = ctx.wait_for(State::Stopped).await;
        trace!("task {} stopped", ctx.id());
        Ok(())
    }

    pub async fn start<T: TaskBuilder<Error = E>>(
        &self,
        task_builder: T,
    ) -> Result<TaskHandle<T::Output, E>, TaskError<E>> {
        let channel_size = task_builder.channel_size();
        let (tx, rx) = channel::<T::Output>(channel_size);
        let task = Box::new(task_builder.build(tx));
        let task_box = TaskBox {
            task,
            ctx: Context::default(),
        };
        let ctx = task_box.ctx.clone();

        self.tx.send(WaitingTask { task: task_box });

        ctx.wait_for(State::Running).await?;

        Ok(TaskHandle { rx, ctx })
    }
}

impl<E: TError> Default for SingletonTask<E> {
    fn default() -> Self {
        Self::new()
    }
}

pub struct TaskHandle<T, E: TError> {
    pub rx: Receiver<T>,
    pub ctx: Context<E>,
}

impl<T, E: TError> TaskHandle<T, E> {
    pub fn stop(self) -> FutureTaskState<E> {
        self.ctx.stop()
    }
    pub fn wait_for_stopped(self) -> impl Future<Output = Result<(), TaskError<E>>> {
        self.ctx.wait_for(State::Stopped)
    }

    /// Receives the next value for this receiver.
    ///
    /// This method returns `None` if the channel has been closed and there are
    /// no remaining messages in the channel's buffer. This indicates that no
    /// further values can ever be received from this `Receiver`. The channel is
    /// closed when all senders have been dropped, or when [`close`] is called.
    ///
    /// If there are no messages in the channel's buffer, but the channel has
    /// not yet been closed, this method will sleep until a message is sent or
    /// the channel is closed.  Note that if [`close`] is called, but there are
    /// still outstanding [`Permits`] from before it was closed, the channel is
    /// not considered closed by `recv` until the permits are released.
    ///
    /// # Cancel safety
    ///
    /// This method is cancel safe. If `recv` is used as the event in a
    /// [`tokio::select!`](crate::select) statement and some other branch
    /// completes first, it is guaranteed that no messages were received on this
    /// channel.
    ///
    /// [`close`]: Self::close
    /// [`Permits`]: struct@crate::sync::mpsc::Permit
    pub async fn recv(&mut self) -> Option<T> {
        self.rx.recv().await
    }

    /// Blocking receive to call outside of asynchronous contexts.
    ///
    /// This method returns `None` if the channel has been closed and there are
    /// no remaining messages in the channel's buffer. This indicates that no
    /// further values can ever be received from this `Receiver`. The channel is
    /// closed when all senders have been dropped, or when [`close`] is called.
    ///
    /// If there are no messages in the channel's buffer, but the channel has
    /// not yet been closed, this method will block until a message is sent or
    /// the channel is closed.
    ///
    /// This method is intended for use cases where you are sending from
    /// asynchronous code to synchronous code, and will work even if the sender
    /// is not using [`blocking_send`] to send the message.
    ///
    /// Note that if [`close`] is called, but there are still outstanding
    /// [`Permits`] from before it was closed, the channel is not considered
    /// closed by `blocking_recv` until the permits are released.
    ///
    /// [`close`]: Self::close
    /// [`Permits`]: struct@crate::sync::mpsc::Permit
    /// [`blocking_send`]: fn@crate::sync::mpsc::Sender::blocking_send
    pub fn blocking_recv(&mut self) -> Option<T> {
        self.rx.blocking_recv()
    }
}
