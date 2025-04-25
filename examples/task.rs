use std::{error::Error, fmt::Display, thread, time::Duration};

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
        write!(f, "{:?}", self)
    }
}

struct Task1 {
    tx: Option<SyncSender<u32>>,
}

impl Task<Error1> for Task1 {
    fn on_start(&mut self, ctx: Context<Error1>) -> LocalBoxFuture<'_, Result<(), Error1>> {
        async move {
            trace!("on_start 1");
            let tx = self.tx.take().unwrap();
            thread::spawn(move || {
                for i in 0..10 {
                    let _ = tx.send(i);
                    thread::sleep(Duration::from_millis(100));
                }
                ctx.stop();
            });

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
    tokio::spawn({
        let st = st.clone();
        async move {
            sleep(Duration::from_millis(200)).await;
            info!("start 2");
            st.start(Tasl1Builder {}).await.unwrap();
        }
    });

    while let Ok(v) = rx.recv() {
        println!("{}", v);
    }

    rx.wait_for_stopped().await.unwrap();

    info!("end");
}
