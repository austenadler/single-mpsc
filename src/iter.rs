use crate::Receiver;
use std::fmt;
use std::iter::FusedIterator;

pub struct Iter<'a, T> {
    pub(crate) receiver: &'a mut Receiver<T>,
}

impl<T> FusedIterator for Iter<'_, T> {}

impl<T> Iterator for Iter<'_, T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.receiver.recv().ok()
    }
}

impl<T> fmt::Debug for Iter<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad("Iter { .. }")
    }
}

pub struct TryIter<'a, T> {
    pub(crate) receiver: &'a mut Receiver<T>,
}

impl<T> Iterator for TryIter<'_, T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.receiver.try_recv().ok()
    }
}

impl<T> fmt::Debug for TryIter<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad("TryIter { .. }")
    }
}

/// A blocking iterator over messages in a channel.
///
/// `next` is blocking and will return `None` when all [`Sender`]s are dropped
///
/// # Example
///
/// ```
/// use std::thread;
/// use single_mpsc::channel;
///
/// let (tx, mut rx) = channel();
///
/// let handle = std::thread::spawn(move || {
///     tx.send("a");
///     drop(tx);
/// });
///
/// // Blocks until tx is dropped
/// let ret: Vec<_> = rx.into_iter().collect();
///
/// assert_eq!(ret, ["a"]);
/// ```

pub struct IntoIter<T> {
    pub(crate) receiver: Receiver<T>,
}

impl<T> FusedIterator for IntoIter<T> {}

impl<T> Iterator for IntoIter<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.receiver.recv().ok()
    }
}

impl<T> fmt::Debug for IntoIter<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad("Iter { .. }")
    }
}
