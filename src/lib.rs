use std::marker::PhantomData;
use std::mem;
use std::ops::{Deref, DerefMut};
use std::ptr;

use buffer::Buffer;
use iter::RawValIter;

mod buffer;
mod iter;

#[derive(Debug)]
pub struct LinearDeque<T> {
    buf: Buffer<T>,
    len: usize,
    off: usize,
}

unsafe impl<T: Send> Send for LinearDeque<T> {}
unsafe impl<T: Sync> Sync for LinearDeque<T> {}

impl<T> LinearDeque<T> {
    fn ptr(&self) -> *mut T {
        self.buf.ptr.as_ptr()
    }

    fn cap(&self) -> usize {
        self.buf.cap
    }

    pub fn new() -> Self {
        LinearDeque {
            buf: Buffer::new(),
            len: 0,
            off: 0,
        }
    }

    pub fn push_back(&mut self, elem: T) {
        self.ensure_reserved_back_space();
        unsafe {
            ptr::write(self.ptr().add(self.off + self.len), elem);
        }

        self.len += 1;
    }

    pub fn pop_back(&mut self) -> Option<T> {
        if self.len == 0 {
            None
        } else {
            self.len -= 1;
            unsafe { Some(ptr::read(self.ptr().add(self.off + self.len))) }
        }
    }

    pub fn insert(&mut self, index: usize, elem: T) {
        assert!(index <= self.len, "index out of bounds");

        if index < self.len / 2 {
            // near front
            todo!()
        } else {
            // near back
            unsafe {
                let pending_copy = self.prepare_reserved_back_space();
                let (front_copy, mut back_copy) = pending_copy.split(index);
                back_copy.dst += 1;
                front_copy.perform(self.ptr());
                back_copy.perform(self.ptr());
                ptr::write(self.ptr().add(self.off + index), elem);
            }
        }
        self.len += 1;
    }

    pub fn remove(&mut self, index: usize) -> Option<T> {
        if index < self.len {
            unsafe {
                let start = self.ptr().add(self.off);
                let result = ptr::read(start.add(index));

                if index < self.len / 2 {
                    // near front
                    ptr::copy(start, start.add(1), index);
                    self.off += 1;
                } else {
                    // near back
                    ptr::copy(start.add(index + 1), start.add(index), self.len - index - 1);
                }
                self.len -= 1;

                Some(result)
            }
        } else {
            None
        }
    }

    // TODO: document that after draining the remaining allocated space is
    // equally splitted as reserved space for front and back.
    pub fn drain(&mut self) -> Drain<T> {
        unsafe {
            let iter = RawValIter::new(self);

            self.len = 0;
            self.off = self.cap() / 2;

            Drain {
                iter,
                vec: PhantomData,
            }
        }
    }

    fn ensure_reserved_back_space(&mut self) {
        unsafe {
            let pending_copy = self.prepare_reserved_back_space();
            pending_copy.perform(self.ptr());
        }
    }

    unsafe fn prepare_reserved_back_space(&mut self) -> PendingCopy {
        let mut pending_copy = PendingCopy {
            src: self.off,
            dst: self.off,
            count: self.len,
        };

        if self.reserved_back_space() > 0 {
            // do nothing
        } else if self.reserved_front_space() > self.len {
            self.off /= 2;
            pending_copy.dst = self.off;
        } else {
            self.buf.realloc(self.cap() + self.len.max(1));
        }

        debug_assert!(self.reserved_back_space() > 0);

        pending_copy
    }

    fn reserved_front_space(&self) -> usize {
        self.off
    }

    fn reserved_back_space(&self) -> usize {
        self.cap() - self.len - self.off
    }
}

impl<T> Default for LinearDeque<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Drop for LinearDeque<T> {
    fn drop(&mut self) {
        while self.pop_back().is_some() {}
    }
}

impl<T> Deref for LinearDeque<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        unsafe { std::slice::from_raw_parts(self.ptr().add(self.off), self.len) }
    }
}

impl<T> DerefMut for LinearDeque<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { std::slice::from_raw_parts_mut(self.ptr().add(self.off), self.len) }
    }
}

impl<T> IntoIterator for LinearDeque<T> {
    type Item = T;

    type IntoIter = IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        unsafe {
            let iter = RawValIter::new(&self);

            let buf = ptr::read(&self.buf);
            mem::forget(self);

            IntoIter { iter, _buf: buf }
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct PendingCopy {
    src: usize,
    dst: usize,
    count: usize,
}

impl PendingCopy {
    unsafe fn perform<T>(self, ptr: *mut T) {
        if self.count > 0 && self.src != self.dst {
            ptr::copy(ptr.add(self.src), ptr.add(self.dst), self.count);
        }
    }

    fn split(self, index: usize) -> (PendingCopy, PendingCopy) {
        let low = PendingCopy {
            count: index,
            ..self
        };
        let high = PendingCopy {
            src: self.src + index,
            dst: self.dst + index,
            count: self.count - index,
        };
        (low, high)
    }
}

pub struct IntoIter<T> {
    _buf: Buffer<T>,
    iter: RawValIter<T>,
}

impl<T> Iterator for IntoIter<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl<T> DoubleEndedIterator for IntoIter<T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.iter.next_back()
    }
}

impl<T> Drop for IntoIter<T> {
    fn drop(&mut self) {
        for _ in &mut *self {}
    }
}

pub struct Drain<'a, T: 'a> {
    vec: PhantomData<&'a mut LinearDeque<T>>,
    iter: RawValIter<T>,
}

impl<'a, T> Iterator for Drain<'a, T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl<'a, T> Drop for Drain<'a, T> {
    fn drop(&mut self) {
        for _ in &mut *self {}
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Debug;
    use std::{mem, ptr};

    use crate::LinearDeque;

    macro_rules! assert_deque {
        ($deque:ident, $expected_reserved_front_space:expr, $expected_elems:expr, $expected_reserved_back_space:expr $(,)?) => {{
            let expected_reserved_front_space = $expected_reserved_front_space;
            let expected_elems = $expected_elems;
            let expected_reserved_back_space = $expected_reserved_back_space;

            let expected_len = expected_elems.len();
            let expected_capacity =
                expected_reserved_front_space + expected_len + expected_reserved_back_space;
            let expected_off = expected_reserved_front_space;

            assert_eq!($deque.cap(), expected_capacity);
            assert_eq!($deque.len, expected_len);
            assert_eq!($deque.off, expected_off);
            for (i, expected_elem) in expected_elems.into_iter().enumerate() {
                let elem = unsafe { ptr::read($deque.ptr().add($deque.off + i)) };
                assert_eq!(elem, expected_elem);
            }
            assert_eq!($deque.reserved_front_space(), expected_reserved_front_space);
            assert_eq!($deque.reserved_back_space(), expected_reserved_back_space);
        }};
    }

    fn prepare_deque<T: Copy + Eq + Debug>(
        reserved_front_space: usize,
        elems: impl IntoIterator<Item = T>,
        reserved_back_space: usize,
    ) -> LinearDeque<T> {
        let elems: Vec<_> = elems.into_iter().collect();
        let capacity = reserved_front_space + elems.len() + reserved_back_space;

        let mut deque: LinearDeque<T> = LinearDeque::new();
        if capacity != 0 {
            deque.buf.realloc(capacity);
            deque.len = elems.len();
            deque.off = reserved_front_space;
            for (i, elem) in elems.iter().enumerate() {
                unsafe {
                    ptr::write(deque.ptr().add(deque.off + i), *elem);
                }
            }
        }

        assert_deque!(deque, reserved_front_space, elems, reserved_back_space);

        deque
    }

    macro_rules! assert_zst_deque {
        ($deque:ident, $expected_len:expr) => {{
            fn assert_zst_deque<T>(_: &LinearDeque<T>) {
                assert_eq!(mem::size_of::<T>(), 0);
            }
            assert_zst_deque(&$deque);
        }
        assert_eq!($deque.cap(), usize::MAX);
        assert_eq!($deque.len, $expected_len);};
    }

    fn prepare_zst_deque<T>(len: usize) -> LinearDeque<T> {
        assert_eq!(mem::size_of::<T>(), 0);
        let mut deque = LinearDeque::new();
        deque.len = len;
        deque
    }

    #[test]
    fn new_zst() {
        let deque: LinearDeque<()> = LinearDeque::new();

        assert_zst_deque!(deque, 0);
    }

    #[test]
    fn push_back_growing() {
        let mut deque: LinearDeque<char> = prepare_deque(0, [], 0);
        // |[]|

        deque.push_back('A');

        // |[A]|
        assert_deque!(deque, 0, ['A'], 0);

        deque.push_back('B');

        // |[AB]|
        assert_deque!(deque, 0, ['A', 'B'], 0);

        deque.push_back('C');

        // |[ABC]-|
        assert_deque!(deque, 0, ['A', 'B', 'C'], 1);

        deque.push_back('D');

        // |[ABCD]|
        assert_deque!(deque, 0, ['A', 'B', 'C', 'D'], 0);

        deque.push_back('E');

        // |[ABCDE]---|
        assert_deque!(deque, 0, ['A', 'B', 'C', 'D', 'E'], 3);
    }

    #[test]
    fn push_back_using_reserved_front_space() {
        let mut deque: LinearDeque<char> = prepare_deque(4, ['A', 'B'], 0);
        // |----[AB]|

        deque.push_back('C');

        // |--[ABC]-|
        assert_deque!(deque, 2, ['A', 'B', 'C'], 1);
    }

    #[test]
    fn push_back_not_using_reserved_front_space() {
        let mut deque: LinearDeque<char> = prepare_deque(1, ['A', 'B'], 0);
        // |-[AB]|

        deque.push_back('C');

        // |-[ABC]-|
        assert_deque!(deque, 1, ['A', 'B', 'C'], 1);

        deque.push_back('D');

        // |-[ABCD]|
        assert_deque!(deque, 1, ['A', 'B', 'C', 'D'], 0);

        deque.push_back('E');

        // |-[ABCDE]---|
        assert_deque!(deque, 1, ['A', 'B', 'C', 'D', 'E'], 3);
    }

    #[test]
    fn push_back_zst() {
        let mut deque = prepare_zst_deque(0);

        deque.push_back(());
        deque.push_back(());

        assert_zst_deque!(deque, 2);
    }

    #[test]
    fn pop_back() {
        let mut deque = prepare_deque(2, ['A', 'B'], 1);
        // |--[AB]-|

        let popped = deque.pop_back();

        assert_eq!(popped, Some('B'));
        // |--[A]--|
        assert_deque!(deque, 2, ['A'], 2);

        let popped = deque.pop_back();

        assert_eq!(popped, Some('A'));
        // |--[]---|
        assert_deque!(deque, 2, [], 3);

        let popped = deque.pop_back();

        assert_eq!(popped, None);
        assert_deque!(deque, 2, [], 3);
    }

    #[test]
    fn pop_back_zst() {
        let mut deque = prepare_zst_deque(2);

        let popped = deque.pop_back();
        assert_eq!(popped, Some(()));
        assert_zst_deque!(deque, 1);

        let popped = deque.pop_back();
        assert_eq!(popped, Some(()));
        assert_zst_deque!(deque, 0);

        let popped = deque.pop_back();
        assert_eq!(popped, None);
        assert_zst_deque!(deque, 0);
    }

    #[test]
    fn insert_near_back_using_reserved_back_space() {
        let mut deque = prepare_deque(1, ['A', 'B', 'C'], 1);
        // |-[ABC]-|

        deque.insert(2, 'x');

        // |-[ABxC]|
        assert_deque!(deque, 1, ['A', 'B', 'x', 'C'], 0);
    }

    #[test]
    fn insert_near_back_reallocating() {
        let mut deque = prepare_deque(1, ['A', 'B', 'C'], 0);
        // |-[ABC]|

        deque.insert(2, 'x');

        // |-[ABxC]--|
        assert_deque!(deque, 1, ['A', 'B', 'x', 'C'], 2);
    }

    #[test]
    fn insert_near_back_using_reserved_front_space() {
        let mut deque = prepare_deque(4, ['A', 'B', 'C'], 0);
        // |----[ABC]|

        deque.insert(2, 'x');

        // |--[ABxC]-|
        assert_deque!(deque, 2, ['A', 'B', 'x', 'C'], 1);
    }

    #[test]
    fn insert_zst() {
        let mut deque = LinearDeque::new();

        deque.insert(0, ());
        deque.insert(1, ());
        deque.insert(2, ());
        deque.insert(2, ());

        assert_zst_deque!(deque, 4);
    }

    #[test]
    fn remove_near_front() {
        let mut deque = prepare_deque(0, ['A', 'B', 'C', 'D'], 0);
        // |[ABCD]|

        let removed = deque.remove(1);

        assert_eq!(removed, Some('B'));
        // |-[ACD]|
        assert_deque!(deque, 1, ['A', 'C', 'D'], 0);
    }

    #[test]
    fn remove_near_back() {
        let mut deque = prepare_deque(0, ['A', 'B', 'C', 'D'], 0);
        // |[ABCD]|

        let removed = deque.remove(2);

        assert_eq!(removed, Some('C'));
        // |[ABD]-|
        assert_deque!(deque, 0, ['A', 'B', 'D'], 1);
    }

    #[test]
    fn remove_zst() {
        let mut deque = prepare_zst_deque(3);

        let removed = deque.remove(1);
        assert_eq!(removed, Some(()));
        assert_zst_deque!(deque, 2);

        let removed = deque.remove(1);
        assert_eq!(removed, Some(()));
        assert_zst_deque!(deque, 1);

        let removed = deque.remove(0);
        assert_eq!(removed, Some(()));
        assert_zst_deque!(deque, 0);

        let removed = deque.remove(0);
        assert_eq!(removed, None);
        assert_zst_deque!(deque, 0);
    }

    #[test]
    fn drain() {
        let mut deque = prepare_deque(2, ['A', 'B', 'C'], 5);
        // |--[ABC]-----|

        let mut iter = deque.drain();

        assert_eq!(iter.next(), Some('A'));
        assert_eq!(iter.next(), Some('B'));
        assert_eq!(iter.next(), Some('C'));
        assert_eq!(iter.next(), None);
        drop(iter);
        // |-----[]-----|
        assert_deque!(deque, 5, [], 5);
    }

    #[test]
    fn drain_zst() {
        let mut deque: LinearDeque<()> = prepare_zst_deque(4);

        let iter = deque.drain();

        assert_eq!(iter.count(), 4);
        assert_zst_deque!(deque, 0);
    }

    #[test]
    fn deref_empty() {
        let deque: LinearDeque<char> = prepare_deque(6, [], 4);

        assert!(deque.is_empty());
        assert_eq!(deque.len(), 0);
    }

    #[test]
    fn deref_non_empty() {
        let deque = prepare_deque(2, ['A', 'B', 'C'], 4);

        assert!(!deque.is_empty());
        assert_eq!(deque.len(), 3);
        assert_eq!(deque[0], 'A');
        assert_eq!(deque[1], 'B');
        assert_eq!(deque[2], 'C');
    }

    #[test]
    fn deref_zst() {
        let deque: LinearDeque<()> = prepare_zst_deque(4);

        assert!(!deque.is_empty());
        assert_eq!(deque.len(), 4);
        assert_eq!(deque[0], ());
        assert_zst_deque!(deque, 4);
    }

    #[test]
    fn into_iter() {
        let deque = prepare_deque(2, ['A', 'B', 'C'], 4);

        let mut iter = deque.into_iter();

        assert_eq!(iter.next(), Some('A'));
        assert_eq!(iter.next(), Some('B'));
        assert_eq!(iter.next(), Some('C'));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn into_iter_double_ended() {
        let deque = prepare_deque(2, ['A', 'B', 'C'], 4);

        let mut iter = deque.into_iter();

        assert_eq!(iter.next_back(), Some('C'));
        assert_eq!(iter.next_back(), Some('B'));
        assert_eq!(iter.next_back(), Some('A'));
        assert_eq!(iter.next_back(), None);
    }

    #[test]
    fn into_iter_mixed() {
        let deque = prepare_deque(2, ['A', 'B', 'C'], 4);

        let mut iter = deque.into_iter();

        assert_eq!(iter.next_back(), Some('C'));
        assert_eq!(iter.next(), Some('A'));
        assert_eq!(iter.next_back(), Some('B'));
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next_back(), None);
    }

    #[test]
    fn into_iter_zst() {
        let deque: LinearDeque<()> = prepare_zst_deque(4);

        let iter = deque.into_iter();

        assert_eq!(iter.count(), 4);
    }
}
