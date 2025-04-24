use std::sync::{Arc, Condvar, Mutex};

pub fn one_channel<T>() -> (Sender<T>, Receiver<T>) {
    let inner = Arc::new(Inner {
        status: Mutex::new(Status {
            queue: None,
            is_stopped: false,
        }),
        not_empty: Condvar::new(),
    });

    let sender = Sender {
        inner: Arc::clone(&inner),
    };

    let receiver = Receiver { inner };

    (sender, receiver)
}

struct Inner<T> {
    status: Mutex<Status<T>>,
    not_empty: Condvar,
}

struct Status<T> {
    queue: Option<T>,
    is_stopped: bool,
}

pub struct Sender<T> {
    inner: Arc<Inner<T>>,
}

impl<T> Sender<T> {
    pub fn send(&self, item: T) -> Option<T> {
        let mut g = self.inner.status.lock().unwrap();
        let old = g.queue.replace(item);
        self.inner.not_empty.notify_one();
        old
    }

    pub fn stop(&self) {
        let mut status = self.inner.status.lock().unwrap();
        status.is_stopped = true;
        self.inner.not_empty.notify_all();
    }
}

pub struct Receiver<T> {
    inner: Arc<Inner<T>>,
}

impl<T> Receiver<T> {
    pub fn recv(&self) -> Option<T> {
        let mut status = self.inner.status.lock().unwrap();
        loop {
            if status.is_stopped {
                return None;
            }
            if let Some(one) = status.queue.take() {
                return Some(one);
            }
            status = self.inner.not_empty.wait(status).unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_one_channel() {
        let (tx, rx) = one_channel();

        let sender_thread = thread::spawn(move || {
            tx.send(1);
            thread::sleep(Duration::from_millis(100));
            tx.send(2);
        });

        let receiver_thread = thread::spawn(move || {
            assert_eq!(rx.recv(), Some(1));
            assert_eq!(rx.recv(), Some(2));
        });

        sender_thread.join().unwrap();
        receiver_thread.join().unwrap();
    }

    #[test]
    fn test_one_channel2() {
        let (tx, rx) = one_channel();

        let sender_thread = thread::spawn(move || {
            tx.send(1);
            let old = tx.send(2);
            assert_eq!(old, Some(1));
        });

        let receiver_thread = thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            assert_eq!(rx.recv(), Some(2));
        });

        sender_thread.join().unwrap();
        receiver_thread.join().unwrap();
    }
    #[test]
    fn test_stop() {
        let (tx, rx) = one_channel();
        tx.send(1);
        tx.stop();

        assert_eq!(rx.recv(), None);
    }
}
