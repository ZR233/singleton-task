use std::{error::Error, fmt::Display, time::Duration};

use log::{LevelFilter, info, trace};
use singleton_task::*;
use tokio::time::sleep;

#[derive(Debug, Clone)]
enum Error1 {
    _A,
}

impl TError for Error1 {}
impl Error for Error1 {}
impl Display for Error1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

struct Task1 {
    tx: Option<SyncSender<u32>>,
}
#[async_trait]
impl Task<Error1> for Task1 {
    async fn on_start(&mut self, ctx: Context<Error1>) -> Result<(), Error1> {
        trace!("[{}]on_start", ctx.id());
        let tx = self.tx.take().unwrap();
        let id = ctx.id();
        ctx.spawn(async move {
            for i in 0..10 {
                let _ = tx.send(i);
                info!("[{id}]send {i}");
                sleep(Duration::from_millis(100)).await;
            }
        });

        Ok(())
    }
}

struct Tasl1Builder {}

impl TaskBuilder for Tasl1Builder {
    type Output = u32;
    type Error = Error1;
    type Task = Task1;

    fn build(self, tx: SyncSender<u32>) -> Self::Task {
        Task1 { tx: Some(tx) }
    }
}

#[tokio::main]
async fn main() {
    env_logger::builder()
        .filter_level(LevelFilter::Trace)
        .init();

    let st = SingletonTask::<Error1>::new();
    let rx = st.start(Tasl1Builder {}).await.unwrap();
    let h2 = tokio::spawn({
        let st = st.clone();
        async move {
            sleep(Duration::from_millis(200)).await;
            info!("start 2");
            st.start(Tasl1Builder {}).await.unwrap();
            info!("start 2 ok");
        }
    });

    while let Ok(v) = rx.recv() {
        println!("{v}");
    }

    assert!(rx.wait_for_stopped().await.is_err());
    info!("task 1 stopped");

    let _ = h2.await;

    info!("end");
}
