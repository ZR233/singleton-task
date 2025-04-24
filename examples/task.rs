use std::{error::Error, fmt::Display};

use log::{LevelFilter, trace};
use singleton_task::*;

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

#[tokio::main]
async fn main() {
    env_logger::builder()
        .filter_level(LevelFilter::Trace)
        .init();

    let st = SingletonTask::<Error1>::new();
    let rx = st.start(Tasl1Builder {}).unwrap();
}
