pub use std::sync::mpsc::{Receiver, SyncSender};
use std::{
    error::Error,
    fmt::Display,
    sync::{Arc, OnceLock, mpsc::sync_channel},
    thread,
};

use context::{FutureTaskState, State};
pub use futures::{FutureExt, future::LocalBoxFuture};
use log::{trace, warn};

mod context;
mod task_chan;

pub use context::Context;
use task_chan::{TaskReceiver, TaskSender, task_channel};
use tokio::{runtime::Runtime, select};

static RT: OnceLock<Runtime> = OnceLock::new();

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
            Self::Error(e) => write!(f, "{}", e),
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

pub trait Task<E: TError>: Send + 'static {
    fn on_start(&mut self, ctx: Context<E>) -> LocalBoxFuture<'_, Result<(), E>> {
        drop(ctx);
        async {
            trace!("on_start");
            Ok(())
        }
        .boxed_local()
    }
    fn on_stop(&mut self, ctx: Context<E>) -> LocalBoxFuture<'_, Result<(), E>> {
        drop(ctx);
        async {
            trace!("on_stop");
            Ok(())
        }
        .boxed_local()
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
    _drop: Arc<TaskDrop<E>>,
}

impl<E: TError> SingletonTask<E> {
    pub fn new() -> Self {
        let (tx, rx) = task_channel::<E>();

        thread::spawn(move || Self::work_deal_start(rx));

        Self {
            _drop: Arc::new(TaskDrop { tx: tx.clone() }),
            tx,
        }
    }

    fn work_deal_start(rx: TaskReceiver<E>) {
        while let Some(next) = rx.recv() {
            let id = next.task.ctx.id();
            if let Err(e) = Self::work_start_task(next) {
                warn!("task [{}] error: {}", id, e);
            }
        }
    }

    fn work_start_task(next: WaitingTask<E>) -> Result<(), TaskError<E>> {
        trace!("run task {}", next.task.ctx.id());
        let ctx = next.task.ctx.clone();
        let mut task = next.task.task;
        match rt().block_on(async {
            select! {
                res = task.on_start(ctx.clone()) => res.map_err(|e|e.into()),
                res = ctx.wait_for(State::Stopping) => res
            }
        }) {
            Ok(_) => {
                if ctx.set_state(State::Running).is_err() {
                    return Err(TaskError::Cancelled);
                };
            }
            Err(e) => {
                ctx.stop_with_result(Some(e));
            }
        }

        rt().block_on(async {
            let _ = ctx.wait_for(State::Stopping).await;
            let _ = task.on_stop(ctx.clone()).await;
            ctx.work_done();
            let _ = ctx.wait_for(State::Stopped).await;
        });

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

struct TaskDrop<E: TError> {
    tx: TaskSender<E>,
}
impl<E: TError> Drop for TaskDrop<E> {
    fn drop(&mut self) {
        self.tx.stop();
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

fn rt() -> &'static Runtime {
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    })
}

#[cfg(test)]
mod test {
    use log::LevelFilter;

    use super::*;

    #[derive(Debug, Clone)]
    enum Error1 {
        _A,
    }

    impl TError for Error1 {}
    impl Error for Error1 {}
    impl Display for Error1 {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{:?}", self)
        }
    }

    struct Task1 {
        _a: i32,
    }

    impl Task<Error1> for Task1 {
        fn on_start(&mut self, _ctx: Context<Error1>) -> LocalBoxFuture<'_, Result<(), Error1>> {
            async {
                trace!("on_start 1");
                Ok(())
            }
            .boxed_local()
        }
    }

    struct Tasl1Builder {}

    impl TaskBuilder for Tasl1Builder {
        type Output = u32;
        type Error = Error1;
        type Task = Task1;

        fn build(self, _tx: SyncSender<u32>) -> Self::Task {
            Task1 { _a: 1 }
        }
    }

    #[tokio::test]
    async fn test_task() {
        env_logger::builder()
            .is_test(true)
            .filter_level(LevelFilter::Trace)
            .init();

        let st = SingletonTask::<Error1>::new();
        let _rx = st.start(Tasl1Builder {}).await.unwrap();
    }
}
