use parking_lot::Condvar;
use parking_lot::Mutex;
use std::error::Error;
use std::fmt;
use std::sync::Arc;

struct Shared<T> {
    inner: Mutex<Option<T>>,
    available: Condvar,
}

pub struct Sender<T> {
    shared: Arc<Shared<T>>,
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        // let mut inner = self.shared.inner.lock();
        if dbg!(Arc::strong_count(&self.shared)) == 2 {
            // If we are holding the last lock, send a notification
            self.shared.available.notify_one();
        }
    }
}

impl<T> Sender<T> {
    pub fn send(&self, t: T) {
        {
            // Set the data and then drop the lock
            *self.shared.inner.lock() = Some(t);
        }

        self.shared.available.notify_one();
    }
}

pub struct Receiver<T> {
    shared: Arc<Shared<T>>,
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub struct RecvError;
impl Error for RecvError {}

impl fmt::Display for RecvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        "receiving on an empty and disconnected channel".fmt(f)
    }
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum TryRecvError {
    Empty,
    Disconnected,
}
impl Error for TryRecvError {}
impl fmt::Display for TryRecvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            TryRecvError::Empty => "receiving on an empty channel".fmt(f),
            TryRecvError::Disconnected => "receiving on an empty and disconnected channel".fmt(f),
        }
    }
}

impl<T> Receiver<T> {
    pub fn recv(&mut self) -> Result<T, RecvError> {
        let mut inner = self.shared.inner.lock();

        loop {
            match inner.take() {
                Some(t) => {
                    return Ok(t);
                }
                None if Arc::strong_count(&self.shared) == 1 => {
                    return Err(RecvError);
                }
                None => {
                    self.shared.available.wait(&mut inner);
                }
            }
        }
    }

    pub fn try_recv(&mut self) -> Result<T, TryRecvError> {
        match self.shared.inner.lock().take() {
            Some(t) => Ok(t),
            None if Arc::strong_count(&self.shared) == 1 => Err(TryRecvError::Disconnected),
            None => Err(TryRecvError::Empty),
        }
    }
}

// impl<T> Iterator for Receiver<T> {
//     type Item = T;

//     fn next(&mut self) -> Option<Self::Item> {
//         self.recv().ok()
//     }
// }

pub fn new_with<T>(maybe_t: Option<T>) -> (Sender<T>, Receiver<T>) {
    let rx = Receiver {
        shared: Arc::new(Shared {
            inner: Mutex::new(maybe_t),
            available: Condvar::new(),
        }),
    };

    let tx = Sender {
        shared: Arc::clone(&rx.shared),
    };

    (tx, rx)
}

pub fn channel_with<T>(t: T) -> (Sender<T>, Receiver<T>) {
    new_with(Some(t))
}

pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    new_with(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ping_pong() {
        let (tx, mut rx) = channel();
        tx.send(42);
        assert_eq!(rx.recv(), Ok(42));
    }

    #[test]
    fn multisend() {
        let (tx, mut rx) = channel();
        tx.send(42);
        tx.send(41);
        assert_eq!(rx.recv(), Ok(41));
        tx.send(40);
        assert_eq!(rx.recv(), Ok(40));
    }

    #[test]
    fn drop_senders() {
        let (tx, mut rx) = channel();
        tx.send(52);
        std::mem::drop(tx);

        assert_eq!(rx.recv(), Ok(52));
        assert_eq!(rx.recv(), Err(RecvError));
    }

    #[test]
    fn multithreaded() {
        let (tx, mut rx) = channel();

        std::thread::spawn(move || {
            tx.send("hi there");
        });

        assert_eq!(rx.recv(), Ok("hi there"));
        // Disconnected
        assert_eq!(rx.recv(), Err(RecvError));
    }

    #[test]
    fn try_recv() {
        let (tx, mut rx) = channel();

        // For syncing
        let sync = Arc::new(Condvar::new());
        let sync_clone = Arc::clone(&sync);
        let mutex = Mutex::new(());
        let mut lock = mutex.lock();

        assert_eq!(rx.try_recv(), Err(TryRecvError::Empty));
        assert_eq!(rx.try_recv(), Err(TryRecvError::Empty));

        std::thread::spawn(move || {
            tx.send("hi there");
            sync_clone.notify_one();
        });

        // Wait until we know it was sent
        sync.wait(&mut lock);

        assert_eq!(rx.try_recv(), Ok("hi there"));
        // Disconnected
        assert_eq!(rx.try_recv(), Err(TryRecvError::Disconnected));
    }
}
