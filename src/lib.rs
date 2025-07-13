pub use std::sync::mpsc::{Receiver, SyncSender};
use std::{
    error::Error,
    fmt::Display,
    sync::mpsc::sync_channel,
};

pub use async_trait::async_trait;
use tokio::select;

use context::{FutureTaskState, State};
use log::{trace, warn};

mod context;
mod task_chan;

pub use context::Context;
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

    fn build(self, tx: SyncSender<Self::Output>) -> Self::Task;
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
        let (tx, rx) = sync_channel::<T::Output>(channel_size);
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

    pub fn recv(&self) -> Result<T, std::sync::mpsc::RecvError> {
        self.rx.recv()
    }
}
