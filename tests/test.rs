use std::{
    error::Error,
    fmt::Display,
    sync::mpsc::RecvError,
    time::{Duration, Instant},
};

use log::{LevelFilter, debug, info, trace};
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

#[async_trait]
impl Task<Error1> for Task1 {
    async fn on_start(&mut self, ctx: Context<Error1>) -> Result<(), Error1> {
        trace!("[{}]on_start", ctx.id());
        let tx = self.tx.take().unwrap();
        let id = ctx.id();
        ctx.spawn(async move {
            for i in 0..10 {
                let _ = tx.send(i);
                info!("[{}]send {}", id, i);
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

fn init_log() {
    let _ = env_logger::builder()
        .filter_level(LevelFilter::Trace)
        .is_test(true)
        .try_init();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_stop() {
    init_log();

    let b = Tasl1Builder {};

    let st = SingletonTask::<Error1>::new();

    let rx = st.start(b).await.unwrap();

    for _ in 0..5 {
        let r = rx.recv().unwrap();
        debug!("rcv  {r}");
    }

    let r = rx.stop().await;

    debug!("stop: {r:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_stop2() {
    init_log();

    let b = Tasl1Builder {};

    let st = SingletonTask::<Error1>::new();

    let rx = st.start(b).await.unwrap();
    let begin = Instant::now();

    let h1 = tokio::spawn(async move {
        for _ in 0..10 {
            let begin = Instant::now();
            match rx.recv() {
                Ok(v) => debug!("rcv  {v}"),
                Err(e) => return Err(e),
            }
            debug!("rcv cost: {:?}", begin.elapsed());
        }
        Ok(())
    });

    let b = Tasl1Builder {};
    sleep(Duration::from_millis(30)).await;

    debug!("start 2, delay {:?}", begin.elapsed());
    let t2 = st.start(b).await.unwrap();

    let r = h1.await.unwrap();
    debug!("h1 end");

    assert!(matches!(r, Err(RecvError)));

    while let Ok(v) = t2.recv() {
        debug!("2 rcv  {v}");
    }

    assert!(true);
}
