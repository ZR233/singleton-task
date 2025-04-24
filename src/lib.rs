use std::{
    error::Error,
    fmt::Display,
    sync::{Arc, Mutex, mpsc::Receiver},
    thread,
};

pub use futures::{FutureExt, future::LocalBoxFuture};
use futures::{executor::block_on, select};
use log::{trace, warn};

mod context;
mod one_channel;

pub use context::Context;

#[derive(Debug, Clone)]
pub enum TaskError<E: Error + Clone> {
    Cancelled,
    Error(E),
}

impl<E: Error + Clone> From<E> for TaskError<E> {
    fn from(value: E) -> Self {
        Self::Error(value)
    }
}

impl<E: Error + Clone> Display for TaskError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => write!(f, "Cancelled"),
            Self::Error(e) => write!(f, "{}", e),
        }
    }
}

pub trait TaskBuilder {
    type Output: Send + 'static;
    type Error: Error + Clone;
    type Task: Task<Self::Error>;

    fn build(self) -> Self::Task;
}

pub trait Task<E: Error + Clone>: Send + 'static {
    fn on_start(&mut self, ctx: Context<E>) -> LocalBoxFuture<'_, Result<(), E>> {
        async {
            trace!("on_start");
            Ok(())
        }
        .boxed_local()
    }
}

struct TaskBox<E: Error + Clone + Send + 'static> {
    task: Box<dyn Task<E>>,
    ctx: Context<E>,
}

struct WaitingTask<E: Error + Clone + Send + 'static> {
    task: TaskBox<E>,
}

pub struct SingletonTask<E: Error + Clone + Send + 'static> {
    tx: one_channel::Sender<WaitingTask<E>>,
    current: Arc<Mutex<Option<Context<E>>>>,
}

impl<E: Error + Clone + Send + 'static> SingletonTask<E> {
    pub fn new() -> Self {
        let current = Arc::new(Mutex::new(None));
        let (tx, rx) = one_channel::one_channel::<WaitingTask<E>>();
        let cur = current.clone();

        thread::spawn(move || Self::work_deal_start(cur, rx));

        Self { tx, current }
    }

    fn work_deal_start(
        current: Arc<Mutex<Option<Context<E>>>>,
        rx: one_channel::Receiver<WaitingTask<E>>,
    ) {
        while let Some(next) = rx.recv() {
            let ctx = next.task.ctx.clone();
            {
                let mut g = current.lock().unwrap();
                if ctx.set_state(context::State::Preparing).is_err() {
                    continue;
                }
                g.replace(next.task.ctx.clone());
            }
            if let Err(e) = Self::work_start_task(next) {
                warn!("task [{}] error: {}", ctx.id(), e);
            }
        }
    }

    fn work_start_task(next: WaitingTask<E>) -> Result<(), TaskError<E>> {
        trace!("run task {}", next.task.ctx.id());
        let res = block_on(async {
            select! {
                res = next.task.task.on_start()=>{
                    res
                },
                _ = ctx.cancel.cancelled()  => {
                    Err()
                }
            }
        });

        Ok(())
    }

    pub fn start<T: TaskBuilder<Error = E>>(
        &self,
        task_builder: T,
    ) -> Result<Receiver<T::Output>, TaskError<E>> {
        let task = Box::new(task_builder.build());
        let task_box = TaskBox {
            task,
            ctx: Context::default(),
        };

        if let Some(old) = self.tx.send(WaitingTask { task: task_box }) {
            trace!("stop old");
        }
        loop {}
        Err(TaskError::Cancelled)
    }
}

impl<E: Error + Clone + Send + 'static> Default for SingletonTask<E> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod test {
    use log::LevelFilter;

    use super::*;

    #[derive(Debug, Clone)]
    enum Error1 {
        A,
    }

    impl Error for Error1 {}
    impl Display for Error1 {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{:?}", self)
        }
    }

    struct Task1 {
        a: i32,
    }

    impl Task<Error1> for Task1 {
        fn on_start(&mut self, ctx: Context<Error1>) -> LocalBoxFuture<'_, Result<(), Error1>> {
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

        fn build(self) -> Self::Task {
            Task1 { a: 1 }
        }
    }

    #[tokio::test]
    async fn test_task() {
        env_logger::builder()
            .is_test(true)
            .filter_level(LevelFilter::Trace)
            .init();

        let st = SingletonTask::<Error1>::new();
        let rx = st.start(Tasl1Builder {}).unwrap();
    }
}
