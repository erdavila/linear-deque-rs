use std::ptr::NonNull;

pub struct DropTracker<T> {
    dropped: Option<Vec<T>>,
}
impl<T: Clone> DropTracker<T> {
    pub fn new() -> Self {
        DropTracker { dropped: None }
    }

    pub fn wrap(&mut self, value: T) -> DropTrackerWrapper<T> {
        DropTrackerWrapper {
            value,
            tracker: NonNull::from(self),
        }
    }

    pub fn wrap_iter<I: IntoIterator<Item = T>>(
        &mut self,
        values: I,
    ) -> WrappedIter<T, I::IntoIter> {
        WrappedIter {
            inner: values.into_iter(),
            tracker: NonNull::from(self),
        }
    }

    pub fn track<F: FnOnce() -> R, R>(&mut self, f: F) -> (Vec<T>, R) {
        self.begin();
        let result = f();
        (self.end(), result)
    }

    pub fn begin(&mut self) {
        self.dropped = Some(Vec::new())
    }

    pub fn end(&mut self) -> Vec<T> {
        self.dropped.take().unwrap()
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DropTrackerWrapper<T: Clone> {
    value: T,
    tracker: NonNull<DropTracker<T>>,
}
impl<T: Clone> Drop for DropTrackerWrapper<T> {
    fn drop(&mut self) {
        let tracker = unsafe { self.tracker.as_mut() };
        if let Some(ref mut dropped) = tracker.dropped {
            dropped.push(self.value.clone());
        }
    }
}

pub struct WrappedIter<T, I: Iterator<Item = T>> {
    inner: I,
    tracker: NonNull<DropTracker<T>>,
}

impl<T: Clone, I: Iterator<Item = T>> Iterator for WrappedIter<T, I> {
    type Item = DropTrackerWrapper<T>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|value| {
            let tracker = unsafe { self.tracker.as_mut() };
            tracker.wrap(value)
        })
    }
}
