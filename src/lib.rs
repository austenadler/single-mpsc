//! Single-value mpsc channel that blocks on `recv`, but doesn't block on `send`
//!
//! This crate has a message capacity of one.
//! Any message sent to this channel will update the latest value and the previous value is dropped.
//!
//! This is similar to the blocking [`std::sync::mpsc::sync_channel`]/`crossbeam::channel::bounded(1)`, except senders will not block when sending messages (except for when acquiring a Mutex lock).
//!
//! A good use-case for this is debouncing expensive code to run only once after any number of sends. For example, when a user is typing, the [`Sender`] part can send each key to the channel and the [`Receiver`] part can fire an expensive function off on each [`Receiver::recv`] call. Since each send just updates the latest value, only 1 value will be available for `recv` at a time
//!
//! # Example
//!
//! ```
//! use single_mpsc::channel;
//!
//! let (tx, mut rx) = channel();
//!
//! // This function takes 30 seconds to run
//! let expensive_fn = |i| {
//!     eprintln!("Expensive function on {i}...");
//!
//!     std::thread::sleep(std::time::Duration::from_secs(30));
//! };
//!
//! // Add 10 events, 1 per second
//! std::thread::spawn(move || {
//!     for i in 0..10 {
//!         tx.send(i);
//!         std::thread::sleep(std::time::Duration::from_secs(1));
//!     }
//!
//!     // Receiver knows when the last transmitter is dropped
//!     std::mem::drop(tx);
//! });
//!
//! while let Ok(msg) = rx.recv() {
//!     // Runs once with the first message 0
//!     // Then, since this function is so slow, runs a second time with the most recent message 10
//!     expensive_fn(msg);
//! }
//! ```

pub mod iter;
use parking_lot::Condvar;
use parking_lot::Mutex;
use std::error::Error;
use std::fmt;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;

struct Shared<T> {
    inner: Mutex<Option<T>>,
    count: AtomicUsize,
    available: Condvar,
}

pub struct Sender<T> {
    shared: Arc<Shared<T>>,
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        self.shared.count.fetch_add(1, Ordering::Relaxed);

        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        // Decrement the number of senders
        self.shared.count.fetch_sub(1, Ordering::Relaxed);

        // Let the receiver know that a sender exited
        self.shared.available.notify_one();
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
                None if self.shared.count.load(Ordering::Relaxed) == 0 => {
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
            None if self.shared.count.load(Ordering::Relaxed) == 0 => {
                Err(TryRecvError::Disconnected)
            }
            None => Err(TryRecvError::Empty),
        }
    }

    pub fn iter(&mut self) -> iter::Iter<'_, T> {
        iter::Iter { receiver: self }
    }
    pub fn try_iter(&mut self) -> iter::TryIter<'_, T> {
        iter::TryIter { receiver: self }
    }
}

impl<T> IntoIterator for Receiver<T> {
    type Item = T;
    type IntoIter = iter::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        iter::IntoIter { receiver: self }
    }
}

fn new_with<T>(maybe_t: Option<T>) -> (Sender<T>, Receiver<T>) {
    let rx = Receiver {
        shared: Arc::new(Shared {
            inner: Mutex::new(maybe_t),
            available: Condvar::new(),
            // Start at 1 transmitter
            count: AtomicUsize::new(1),
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
    use std::time::Duration;

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
        let (tx, mut rx) = channel_with("asdf");

        // For syncing
        let sync = Arc::new(Condvar::new());
        let sync_clone = Arc::clone(&sync);
        let mutex = Mutex::new(());
        let mut lock = mutex.lock();

        assert_eq!(rx.try_recv(), Ok("asdf"));

        assert_eq!(rx.try_recv(), Err(TryRecvError::Empty));
        assert_eq!(rx.try_recv(), Err(TryRecvError::Empty));

        let handle = std::thread::spawn(move || {
            tx.send("hi there");
            sync_clone.notify_one();
        });

        // Wait until we know it was sent
        sync.wait(&mut lock);

        assert_eq!(rx.try_recv(), Ok("hi there"));
        // Disconnected
        assert_eq!(rx.try_recv(), Err(TryRecvError::Disconnected));

        // Ensure the thread stopped
        handle.join().unwrap();
    }

    #[test]
    fn multiple_senders() {
        let (tx, mut rx) = channel();
        let tx2 = tx.clone();

        tx.send(1);
        assert_eq!(rx.recv(), Ok(1));

        tx2.send(3);
        assert_eq!(rx.recv(), Ok(3));

        tx.send(1);
        tx2.send(2);
        tx2.send(4);
        tx.send(5);
        assert_eq!(rx.recv(), Ok(5));
        assert_eq!(rx.try_recv(), Err(TryRecvError::Empty));

        std::mem::drop(tx);
        assert_eq!(rx.try_recv(), Err(TryRecvError::Empty));

        std::mem::drop(tx2);
        assert_eq!(rx.try_recv(), Err(TryRecvError::Disconnected));
    }

    #[test]
    fn sender_was_dropped() {
        let (tx, mut rx) = channel::<()>();

        let handle = std::thread::spawn(move || {
            // Ensure the channel is empty right now
            assert_eq!(rx.try_recv(), Err(TryRecvError::Empty));

            // Wait a second and make sure it's still empty
            std::thread::sleep(Duration::from_secs(1));
            assert_eq!(rx.try_recv(), Err(TryRecvError::Empty));

            // Should wait about 2 more seconds, then get a notification that the last sender was dropped
            assert_eq!(rx.recv(), Err(RecvError));
        });

        std::thread::sleep(Duration::from_secs(3));

        std::mem::drop(tx);

        // Wait to make sure the thread returns
        // Assert okay because if error, that means it paniced (assertion failed)
        handle.join().unwrap();
    }
}
