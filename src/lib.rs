#![warn(missing_docs)]
#![doc(test(attr(deny(warnings))))]

//! A double-ended queue that can be sliced at any time without preparation.
//!
//! # [`LinearDeque`] vs [`VecDeque`]
//!
//! ## Slicing
//!
//! The standard [`VecDeque`] uses a ring buffer. It requires that the
//! [`make_contiguous`] method is called to ensure that the deque content can
//! all be referenced in a single slice. [`make_contiguous`] is only callable on
//! a mutable instance of the deque.
//!
//! The [`LinearDeque`] provided by this lib uses a linear buffer, keeping all
//! its content contiguous and allowing to have a slice with all the content at
//! any time, even when the deque is not mutable.
//!
//! ## Memory Usage
//!
//! By using a ring buffer, all spare memory allocated by the standard
//! [`VecDeque`] can be used for elements added at both the front and the back
//! ends.
//!
//! Using a linear buffer, each end of the [`LinearDeque`] have its own reserved
//! memory, so it tends to use more memory than [`VecDeque`].
//!
//! [`VecDeque`]: std::collections::VecDeque
//! [`make_contiguous`]: std::collections::VecDeque::make_contiguous

use std::hash::Hash;
use std::marker::PhantomData;
use std::mem;
use std::ops::{Deref, DerefMut};
use std::ptr;

use buffer::Buffer;
use iter::RawValIter;

mod buffer;
mod iter;

#[cfg(test)]
mod drop_tracker;

/// A double-ended queue implemented with a growable linear buffer.
///
/// A `LinearDeque` with a known list of items can be initialized from an array:
///
/// ```
/// use linear_deque::LinearDeque;
///
/// # #[allow(unused)]
/// let deq = LinearDeque::from([-1, 0, 1]);
/// ```
///
/// Since `LinearDeque` is a linear buffer, its elements are contiguous in
/// memory, and it can be coerced into a slice at any time.
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

    /// Creates an empty deque.
    ///
    /// # Example
    ///
    /// ```
    /// use linear_deque::LinearDeque;
    /// # #[allow(unused)]
    /// let deque: LinearDeque<u32> = LinearDeque::new();
    /// ```
    pub fn new() -> Self {
        Self::with_reserved_space(0, 0)
    }

    /// Creates an empty deque with reserved spaces at the front and the back.
    ///
    /// # Example
    ///
    /// ```
    /// use linear_deque::LinearDeque;
    ///
    /// let deque: LinearDeque<u32> = LinearDeque::with_reserved_space(3, 7);
    ///
    /// assert_eq!(deque.reserved_front_space(), 3);
    /// assert_eq!(deque.reserved_back_space(), 7);
    /// assert_eq!(deque.len(), 0);
    /// ```
    pub fn with_reserved_space(front: usize, back: usize) -> Self {
        let mut buf = Buffer::new();

        let cap = front + back;
        if cap > 0 && !is_zst::<T>() {
            buf.realloc(cap);
        }

        LinearDeque {
            buf,
            len: 0,
            off: front,
        }
    }

    /// Provides a reference to the front element, or `None` if the deque is
    /// empty.
    ///
    /// # Example
    ///
    /// ```
    /// use linear_deque::LinearDeque;
    ///
    /// let mut d = LinearDeque::new();
    /// assert_eq!(d.front(), None);
    ///
    /// d.push_back(1);
    /// d.push_back(2);
    /// assert_eq!(d.front(), Some(&1));
    /// ```
    pub fn front(&self) -> Option<&T> {
        self.first()
    }

    /// Provides a mutable reference to the front element, or `None` if the
    /// deque is empty.
    ///
    /// # Example
    ///
    /// ```
    /// use linear_deque::LinearDeque;
    ///
    /// let mut d = LinearDeque::new();
    /// assert_eq!(d.front_mut(), None);
    ///
    /// d.push_back(1);
    /// d.push_back(2);
    /// match d.front_mut() {
    ///     Some(x) => *x = 9,
    ///     None => (),
    /// }
    /// assert_eq!(d.front(), Some(&9));
    /// ```
    pub fn front_mut(&mut self) -> Option<&mut T> {
        self.first_mut()
    }

    /// Provides a reference to the back element, or `None` if the deque is
    /// empty.
    ///
    /// # Example
    ///
    /// ```
    /// use linear_deque::LinearDeque;
    ///
    /// let mut d = LinearDeque::new();
    /// assert_eq!(d.back(), None);
    ///
    /// d.push_back(1);
    /// d.push_back(2);
    /// assert_eq!(d.back(), Some(&2));
    /// ```
    pub fn back(&self) -> Option<&T> {
        self.last()
    }

    /// Provides a mutable reference to the back element, or `None` if the
    /// deque is empty.
    ///
    /// # Example
    ///
    /// ```
    /// use linear_deque::LinearDeque;
    ///
    /// let mut d = LinearDeque::new();
    /// assert_eq!(d.back(), None);
    ///
    /// d.push_back(1);
    /// d.push_back(2);
    /// match d.back_mut() {
    ///     Some(x) => *x = 9,
    ///     None => (),
    /// }
    /// assert_eq!(d.back(), Some(&9));
    /// ```
    pub fn back_mut(&mut self) -> Option<&mut T> {
        self.last_mut()
    }

    /// Prepends an element to the deque.
    ///
    /// # Example
    ///
    /// ```
    /// use linear_deque::LinearDeque;
    ///
    /// let mut d = LinearDeque::new();
    /// d.push_front(1);
    /// d.push_front(2);
    /// assert_eq!(d.front(), Some(&2));
    /// ```
    pub fn push_front(&mut self, elem: T) {
        if !is_zst::<T>() {
            self.ensure_reserved_front_space();
            unsafe {
                self.off -= 1;
                ptr::write(self.ptr().add(self.off), elem);
            }
        }
        self.len += 1;
    }

    /// Appends an element to the back of the deque.
    ///
    /// # Example
    ///
    /// ```
    /// use linear_deque::LinearDeque;
    ///
    /// let mut buf = LinearDeque::new();
    /// buf.push_back(1);
    /// buf.push_back(3);
    /// assert_eq!(3, *buf.back().unwrap());
    /// ```
    pub fn push_back(&mut self, elem: T) {
        self.ensure_reserved_back_space();
        unsafe {
            ptr::write(self.ptr().add(self.off + self.len), elem);
        }

        self.len += 1;
    }

    /// Removes the first element and returns it, or `None` if the deque is
    /// empty.
    ///
    /// # Example
    ///
    /// ```
    /// use linear_deque::LinearDeque;
    ///
    /// let mut d = LinearDeque::new();
    /// d.push_back(1);
    /// d.push_back(2);
    ///
    /// assert_eq!(d.pop_front(), Some(1));
    /// assert_eq!(d.pop_front(), Some(2));
    /// assert_eq!(d.pop_front(), None);
    /// ```
    pub fn pop_front(&mut self) -> Option<T> {
        if self.len == 0 {
            None
        } else {
            self.len -= 1;
            self.off += 1;
            unsafe { Some(ptr::read(self.ptr().add(self.off - 1))) }
        }
    }

    /// Removes the last element from the deque and returns it, or `None` if
    /// it is empty.
    ///
    /// # Example
    ///
    /// ```
    /// use linear_deque::LinearDeque;
    ///
    /// let mut buf = LinearDeque::new();
    /// assert_eq!(buf.pop_back(), None);
    /// buf.push_back(1);
    /// buf.push_back(3);
    /// assert_eq!(buf.pop_back(), Some(3));
    /// ```
    pub fn pop_back(&mut self) -> Option<T> {
        if self.len == 0 {
            None
        } else {
            self.len -= 1;
            unsafe { Some(ptr::read(self.ptr().add(self.off + self.len))) }
        }
    }

    /// Inserts an element at `index` within the deque, shifting all elements
    /// before or after the index.
    ///
    /// If `index` is nearer to the front, the elements with indices lower than
    /// `index` are moved to the left; otherwise, the elements with indices
    /// grater than `index` are moved right.
    ///
    /// Element at index 0 is the front of the queue.
    ///
    /// # Panics
    ///
    /// Panics if `index` is greater than deque's length
    ///
    /// # Example
    ///
    /// ```
    /// use linear_deque::LinearDeque;
    ///
    /// let mut deque = LinearDeque::new();
    /// deque.push_back('a');
    /// deque.push_back('b');
    /// deque.push_back('c');
    /// assert_eq!(deque, &['a', 'b', 'c']);
    ///
    /// deque.insert(1, 'd');
    /// assert_eq!(deque, &['a', 'd', 'b', 'c']);
    /// ```
    pub fn insert(&mut self, index: usize, elem: T) {
        assert!(index <= self.len, "index out of bounds");

        if !is_zst::<T>() {
            if 2 * index < self.len {
                // near front
                unsafe {
                    let pending_copy = self.prepare_reserved_front_space();
                    let (mut front_copy, back_copy) = pending_copy.split(index);
                    front_copy.dst -= 1;
                    back_copy.perform(self.ptr());
                    front_copy.perform(self.ptr());
                    self.off -= 1;
                    ptr::write(self.ptr().add(self.off + index), elem);
                }
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
        }
        self.len += 1;
    }

    /// Removes and returns the element at `index` from the deque.
    /// Whichever end is closer to the removal point will be moved to make
    /// room, and all the affected elements will be moved to new positions.
    /// Returns `None` if `index` is out of bounds.
    ///
    /// Element at index 0 is the front of the queue.
    ///
    /// # Example
    ///
    /// ```
    /// use linear_deque::LinearDeque;
    ///
    /// let mut buf = LinearDeque::new();
    /// buf.push_back(1);
    /// buf.push_back(2);
    /// buf.push_back(3);
    /// assert_eq!(buf, [1, 2, 3]);
    ///
    /// assert_eq!(buf.remove(1), Some(2));
    /// assert_eq!(buf, [1, 3]);
    /// ```
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

    /// Removes all elements from the deque in bulk, returning all removed
    /// elements as an iterator. If the iterator is dropped before being fully
    /// consumed, it drops the remaining removed elements.
    ///
    /// The returned iterator keeps a mutable borrow on the queue to optimize
    /// its implementation.
    ///
    /// After draining, the remaining unused allocated memory is equaly split
    /// as reserved space for both ends.
    ///
    /* TODO: range parameter is not implemented.
    /// # Panics
    ///
    /// Panics if the starting point is greater than the end point or if
    /// the end point is greater than the length of the deque.
     */
    ///
    /// # Leaking
    ///
    /// If the returned iterator goes out of scope without being dropped (due to
    /// [`mem::forget`], for example), the deque may have lost and leaked
    /// elements arbitrarily, including elements outside the range.
    /* TODO: range parameter.
    ///
    /// # Example
    ///
    /// ```
    /// use linear_deque::LinearDeque;
    ///
    /// let mut deque: LinearDeque<_> = [1, 2, 3].into();
    /// let drained = deque.drain(2..).collect::<LinearDeque<_>>();
    /// assert_eq!(drained, [3]);
    /// assert_eq!(deque, [1, 2]);
    ///
    /// // A full range clears all contents, like `clear()` does
    /// deque.drain(..);
    /// assert!(deque.is_empty());
    /// ```
     */
    // TODO: implement range parameter.
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

    /// Shortens the deque, keeping the last `len` elements and dropping
    /// the rest.
    ///
    /// If `len` is greater than the deque's current length, this has no
    /// effect.
    ///
    /// # Example
    ///
    /// ```
    /// use linear_deque::LinearDeque;
    ///
    /// let mut buf = LinearDeque::new();
    /// buf.push_back(5);
    /// buf.push_back(10);
    /// buf.push_back(15);
    /// assert_eq!(buf, [5, 10, 15]);
    /// buf.truncate_at_front(1);
    /// assert_eq!(buf, [15]);
    /// ```
    pub fn truncate_at_front(&mut self, len: usize) {
        if len < self.len {
            unsafe {
                let count = self.len() - len;
                let front = self.get_unchecked_mut(..count);
                ptr::drop_in_place(front);
                self.off += count;
                self.len = len;
            }
        }
    }

    /// Shortens the deque, keeping the first `len` elements and dropping
    /// the rest.
    ///
    /// If `len` is greater than the deque's current length, this has no
    /// effect.
    ///
    /// # Example
    ///
    /// ```
    /// use linear_deque::LinearDeque;
    ///
    /// let mut buf = LinearDeque::new();
    /// buf.push_back(5);
    /// buf.push_back(10);
    /// buf.push_back(15);
    /// assert_eq!(buf, [5, 10, 15]);
    /// buf.truncate_at_back(1);
    /// assert_eq!(buf, [5]);
    /// ```
    pub fn truncate_at_back(&mut self, len: usize) {
        if len < self.len {
            unsafe {
                let back = self.get_unchecked_mut(len..);
                ptr::drop_in_place(back);
                self.len = len;
            }
        }
    }

    /// Shortens the deque, keeping the first `len` elements and dropping
    /// the rest.
    ///
    /// It is just an alias for [`truncate_at_back`].
    ///
    /// [`truncate_at_back`]: LinearDeque::truncate_at_back
    #[inline]
    pub fn truncate(&mut self, len: usize) {
        self.truncate_at_back(len);
    }

    /// Clears the deque, removing all values.
    ///
    /// After clearing, the reserved space is equally distributed to the front
    /// and back ends.
    ///
    /// # Example
    ///
    /// ```
    /// use linear_deque::LinearDeque;
    ///
    /// let mut deque = LinearDeque::new();
    /// deque.push_back(1);
    /// deque.clear();
    /// assert!(deque.is_empty());
    /// ```
    pub fn clear(&mut self) {
        self.truncate_at_back(0);
        self.off = self.cap() / 2;
    }

    /// Sets the reserved space on both ends of the deque.
    ///
    /// When the reserved front space is changed, the existing elements are moved.
    ///
    /// # Example
    ///
    /// ```
    /// use linear_deque::{LinearDeque, SetReservedSpace};
    ///
    /// let mut deque: LinearDeque<i32> = LinearDeque::with_reserved_space(3, 7);
    /// assert_eq!(deque.reserved_front_space(), 3);
    /// assert_eq!(deque.reserved_back_space(), 7);
    ///
    /// deque.set_reserved_space(SetReservedSpace::GrowTo(8), SetReservedSpace::Keep);
    /// assert_eq!(deque.reserved_front_space(), 8);
    /// assert_eq!(deque.reserved_back_space(), 7);
    /// ```
    pub fn set_reserved_space(&mut self, front: SetReservedSpace, back: SetReservedSpace) {
        if is_zst::<T>() {
            return;
        }

        let front_space = self.reserved_front_space();
        let back_space = self.reserved_back_space();
        let cap = self.cap();

        fn new_space(current: usize, set: SetReservedSpace) -> usize {
            match set {
                SetReservedSpace::Keep => current,
                SetReservedSpace::ShrinkTo(new) => current.min(new),
                SetReservedSpace::GrowTo(new) => current.max(new),
                SetReservedSpace::Exact(new) => new,
            }
        }

        let new_front_space = new_space(front_space, front);
        let new_back_space = new_space(back_space, back);
        let new_cap = new_front_space + self.len + new_back_space;

        if new_cap > cap {
            self.buf.realloc(new_cap);
        }

        if new_front_space != front_space {
            unsafe {
                ptr::copy(
                    self.ptr().add(front_space),
                    self.ptr().add(new_front_space),
                    self.len,
                );
            }
            self.off = new_front_space;
        }

        if new_cap < cap {
            self.buf.realloc(new_cap);
        }
    }

    fn ensure_reserved_front_space(&mut self) {
        unsafe {
            let pending_copy = self.prepare_reserved_front_space();
            pending_copy.perform(self.ptr());
        }
    }

    unsafe fn prepare_reserved_front_space(&mut self) -> PendingCopy {
        let mut pending_copy = PendingCopy {
            src: self.off,
            dst: self.off,
            count: self.len,
        };

        if self.reserved_front_space() > 0 {
            // do nothing
        } else if self.reserved_back_space() > self.len {
            let moved_space = self.reserved_back_space() / 2;
            pending_copy.dst += moved_space;
            self.off += moved_space;
        } else {
            let added_space = self.len.max(1);
            self.buf.realloc(self.cap() + added_space);
            pending_copy.dst += added_space;
            self.off += added_space;
        }

        debug_assert!(self.reserved_front_space() > 0);

        pending_copy
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

    /// Returns the number of elements can be put at the front of the deque
    /// without reallocating or moving existing elements.
    ///
    /// # Example
    ///
    /// ```
    /// use linear_deque::LinearDeque;
    ///
    /// let buf: LinearDeque<i32> = LinearDeque::with_reserved_space(7, 3);
    /// assert!(buf.reserved_front_space() == 7);
    /// ```
    pub fn reserved_front_space(&self) -> usize {
        if is_zst::<T>() {
            usize::MAX
        } else {
            self.off
        }
    }

    /// Returns the number of elements can be put at the back of the deque
    /// without reallocating or moving existing elements.
    ///
    /// # Example
    ///
    /// ```
    /// use linear_deque::LinearDeque;
    ///
    /// let buf: LinearDeque<i32> = LinearDeque::with_reserved_space(7, 3);
    /// assert!(buf.reserved_back_space() == 3);
    /// ```
    pub fn reserved_back_space(&self) -> usize {
        if is_zst::<T>() {
            usize::MAX
        } else {
            self.cap() - self.len - self.off
        }
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

macro_rules! impl_partial_eq {
    ([$($n:tt)*] $rhs:ty) => {
        impl<T, U, $($n)*> PartialEq<$rhs> for LinearDeque<T>
        where
            T: PartialEq<U>,
        {
            fn eq(&self, other: & $rhs) -> bool {
                self.deref() == other.deref()
            }
        }
    };
}

impl_partial_eq!([const N: usize] [U; N]);
impl_partial_eq!([const N: usize] &[U; N]);
impl_partial_eq!([const N: usize] &mut [U; N]);
impl_partial_eq!([] & [U]);
impl_partial_eq!([] &mut [U]);
impl_partial_eq!([] Vec<U>);
impl_partial_eq!([] LinearDeque<U>);

impl<T: Eq> Eq for LinearDeque<T> {}

impl<T: PartialOrd> PartialOrd for LinearDeque<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.iter().partial_cmp(other.iter())
    }
}

impl<T: Ord> Ord for LinearDeque<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.iter().cmp(other.iter())
    }
}

impl<T: Hash> Hash for LinearDeque<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.deref().hash(state);
    }
}

impl<T, const N: usize> From<[T; N]> for LinearDeque<T> {
    /// Converts a `[T; N]` into a `LinearDeque<T>`.
    ///
    /// ```
    /// use linear_deque::LinearDeque;
    ///
    /// let deq = LinearDeque::from([1, 2, 3, 4]);
    /// assert_eq!(deq, [1, 2, 3, 4]);
    /// ```
    fn from(value: [T; N]) -> Self {
        Self::from_iter(value)
    }
}

impl<T> From<Vec<T>> for LinearDeque<T> {
    /// Turn a [`Vec<T>`] into a [`LinearDeque<T>`].
    fn from(value: Vec<T>) -> Self {
        Self::from_iter(value)
    }
}

impl<T> FromIterator<T> for LinearDeque<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let iter = iter.into_iter();
        let (lower, upper) = iter.size_hint();
        let size = upper.unwrap_or(lower);
        let mut deque = Self::with_reserved_space(0, size);
        for elem in iter {
            deque.push_back(elem);
        }
        deque
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

/// Defines what happens to the reserved space of one of the deque ends on a call
/// to [`set_reserved_space`].
///
/// [`set_reserved_space`]: LinearDeque::set_reserved_space
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SetReservedSpace {
    /// The reserved space is not changed.
    Keep,

    /// The reserved space is shrunk to the specified size.
    ///
    /// If the current space is less than the wanted size, then this is a no-op.
    ShrinkTo(usize),

    /// The reserved space is grown to the specified size.
    ///
    /// If the current space is greater than the wanted size, then this is a no-op.
    GrowTo(usize),

    /// The reserved space is unconditionally changed to the specified size.
    Exact(usize),
}

/// An owning iterator over the elements of a `LinearDeque`.
///
/// This `struct` is created by the [`into_iter`] method on [`LinearDeque`]
/// (provided by the [`IntoIterator`] trait). See its documentation for more.
///
/// [`into_iter`]: LinearDeque::into_iter
/// [`IntoIterator`]: core::iter::IntoIterator
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

/// A draining iterator over the elements of a `LinearDeque`.
///
/// This `struct` is created by the [`drain`] method on [`LinearDeque`]. See its
/// documentation for more.
///
/// [`drain`]: LinearDeque::drain
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

fn is_zst<T>() -> bool {
    mem::size_of::<T>() == 0
}

#[cfg(test)]
mod tests {
    use std::collections::hash_map::DefaultHasher;
    use std::fmt::Debug;
    use std::hash::{Hash, Hasher};
    use std::{mem, ptr};

    use crate::drop_tracker::DropTracker;
    use crate::{LinearDeque, SetReservedSpace};

    macro_rules! assert_deque {
        ($deque:ident, $expected_reserved_front_space:expr, $expected_elems:expr, $expected_reserved_back_space:expr $(,)?) => {{
            let expected_reserved_front_space = $expected_reserved_front_space;
            let expected_elems: Vec<_> = $expected_elems.into_iter().collect();
            let expected_reserved_back_space = $expected_reserved_back_space;

            let expected_len = expected_elems.len();
            let expected_capacity =
                expected_reserved_front_space + expected_len + expected_reserved_back_space;
            let expected_off = expected_reserved_front_space;

            assert_eq!($deque.cap(), expected_capacity, "cap");
            assert_eq!($deque.len, expected_len, "len");
            assert_eq!($deque.off, expected_off, "off");
            for (i, expected_elem) in expected_elems.into_iter().enumerate() {
                let elem = unsafe { ptr::read($deque.ptr().add($deque.off + i)) };
                assert_eq!(elem, expected_elem, "index {i}");
            }
            assert_eq!(
                $deque.reserved_front_space(),
                expected_reserved_front_space,
                "front space"
            );
            assert_eq!(
                $deque.reserved_back_space(),
                expected_reserved_back_space,
                "back_space"
            );
        }};
    }

    fn prepare_deque<T: Clone + Eq + Debug>(
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
                    ptr::write(deque.ptr().add(deque.off + i), elem.clone());
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
    fn with_reserved_space() {
        let deque: LinearDeque<char> = LinearDeque::with_reserved_space(3, 4);

        assert_deque!(deque, 3, [], 4);
    }

    #[test]
    fn with_reserved_space_zst() {
        let deque: LinearDeque<()> = LinearDeque::with_reserved_space(3, 4);

        assert_zst_deque!(deque, 0);
    }

    #[test]
    fn front_zst() {
        let deque: LinearDeque<()> = prepare_zst_deque(0);
        assert_eq!(deque.front(), None);

        let deque: LinearDeque<()> = prepare_zst_deque(2);
        assert_eq!(deque.front(), Some(&()));
    }

    #[test]
    fn back_zst() {
        let deque: LinearDeque<()> = prepare_zst_deque(0);
        assert_eq!(deque.back(), None);

        let deque: LinearDeque<()> = prepare_zst_deque(2);
        assert_eq!(deque.back(), Some(&()));
    }

    #[test]
    fn push_front_growing() {
        let mut deque: LinearDeque<char> = prepare_deque(0, [], 0);
        // |[]|

        deque.push_front('A');

        // |[A]|
        assert_deque!(deque, 0, ['A'], 0);

        deque.push_front('B');

        // |[BA]|
        assert_deque!(deque, 0, ['B', 'A'], 0);

        deque.push_front('C');

        // |-[CBA]|
        assert_deque!(deque, 1, ['C', 'B', 'A'], 0);

        deque.push_front('D');

        // |[DCBA]|
        assert_deque!(deque, 0, ['D', 'C', 'B', 'A'], 0);

        deque.push_front('E');

        // |---[EDCBA]|
        assert_deque!(deque, 3, ['E', 'D', 'C', 'B', 'A'], 0);
    }

    #[test]
    fn push_front_using_reserved_back_space() {
        let mut deque: LinearDeque<char> = prepare_deque(0, ['B', 'A'], 4);
        // |[BA]----|

        deque.push_front('C');

        // |-[CBA]--|
        assert_deque!(deque, 1, ['C', 'B', 'A'], 2);
    }

    #[test]
    fn push_front_not_using_reserved_back_space() {
        let mut deque: LinearDeque<char> = prepare_deque(0, ['B', 'A'], 1);
        // |[BA]-|

        deque.push_front('C');

        // |-[CBA]-|
        assert_deque!(deque, 1, ['C', 'B', 'A'], 1);

        deque.push_front('D');

        // |[DCBA]-|
        assert_deque!(deque, 0, ['D', 'C', 'B', 'A'], 1);

        deque.push_front('E');

        // |---[EDCBA]-|
        assert_deque!(deque, 3, ['E', 'D', 'C', 'B', 'A'], 1);
    }

    #[test]
    fn push_front_zst() {
        let mut deque = prepare_zst_deque(0);

        deque.push_front(());
        deque.push_front(());

        assert_zst_deque!(deque, 2);
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
    fn pop_front() {
        let mut deque = prepare_deque(1, ['B', 'A'], 2);
        // |-[AB]--|

        let popped = deque.pop_front();

        assert_eq!(popped, Some('B'));
        // |--[A]--|
        assert_deque!(deque, 2, ['A'], 2);

        let popped = deque.pop_front();

        assert_eq!(popped, Some('A'));
        // |---[]--|
        assert_deque!(deque, 3, [], 2);

        let popped = deque.pop_front();

        assert_eq!(popped, None);
        assert_deque!(deque, 3, [], 2);
    }

    #[test]
    fn pop_front_zst() {
        let mut deque = prepare_zst_deque(2);

        let popped = deque.pop_front();
        assert_eq!(popped, Some(()));
        assert_zst_deque!(deque, 1);

        let popped = deque.pop_front();
        assert_eq!(popped, Some(()));
        assert_zst_deque!(deque, 0);

        let popped = deque.pop_front();
        assert_eq!(popped, None);
        assert_zst_deque!(deque, 0);
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
    fn insert_near_front_using_reserved_front_space() {
        let mut deque = prepare_deque(1, ['A', 'B', 'C'], 1);
        // |-[ABC]-|

        deque.insert(1, 'x');

        // |[AxBC]-|
        assert_deque!(deque, 0, ['A', 'x', 'B', 'C'], 1);
    }

    #[test]
    fn insert_near_front_reallocating() {
        let mut deque = prepare_deque(0, ['A', 'B', 'C'], 1);
        // |[ABC]-|

        deque.insert(1, 'x');

        // |--[AxBC]-|
        assert_deque!(deque, 2, ['A', 'x', 'B', 'C'], 1);
    }

    #[test]
    fn insert_near_front_using_reserved_back_space() {
        let mut deque = prepare_deque(0, ['A', 'B', 'C'], 4);
        // |[ABC]----|

        deque.insert(1, 'x');

        // |-[AxBC]--|
        assert_deque!(deque, 1, ['A', 'x', 'B', 'C'], 2);
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
        deque.insert(0, ());
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
    fn reserved_front_space() {
        let deque = prepare_deque(3, 'A'..='D', 4);

        assert_eq!(deque.reserved_front_space(), 3);
    }

    #[test]
    fn reserved_front_space_zst() {
        let deque: LinearDeque<()> = prepare_zst_deque(5);

        assert_eq!(deque.reserved_front_space(), usize::MAX);
    }

    #[test]
    fn reserved_back_space() {
        let deque = prepare_deque(3, 'A'..='D', 4);

        assert_eq!(deque.reserved_back_space(), 4);
    }

    #[test]
    fn reserved_back_space_zst() {
        let deque: LinearDeque<()> = prepare_zst_deque(5);

        assert_eq!(deque.reserved_back_space(), usize::MAX);
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

    #[test]
    fn truncate_at_front() {
        let mut drop_tracker = DropTracker::new();
        let mut deque = prepare_deque(5, drop_tracker.wrap_iter('A'..='F'), 1);
        // |-----[ABCDEF]-|

        let (dropped, _) = drop_tracker.track(|| {
            deque.truncate_at_front(2);
        });

        // |---------[EF]-|
        assert_deque!(deque, 9, drop_tracker.wrap_iter('E'..='F'), 1);
        assert_eq!(dropped, Vec::from_iter('A'..='D'));
    }

    #[test]
    fn truncate_at_front_zst() {
        let mut deque: LinearDeque<()> = prepare_zst_deque(4);

        deque.truncate_at_front(3);

        assert_zst_deque!(deque, 3);
    }

    #[test]
    fn truncate_at_back() {
        let mut drop_tracker = DropTracker::new();
        let mut deque = prepare_deque(1, drop_tracker.wrap_iter('A'..='F'), 5);
        // |-[ABCDEF]-----|

        let (dropped, _) = drop_tracker.track(|| {
            deque.truncate_at_back(2);
        });

        // |-[AB]---------|
        assert_deque!(deque, 1, drop_tracker.wrap_iter('A'..='B'), 9);
        assert_eq!(dropped, Vec::from_iter('C'..='F'));
    }

    #[test]
    fn truncate_at_back_zst() {
        let mut deque: LinearDeque<()> = prepare_zst_deque(4);

        deque.truncate_at_back(3);

        assert_zst_deque!(deque, 3);
    }

    #[test]
    fn clear() {
        let mut deque = prepare_deque(2, 'A'..='D', 4);
        // |--[ABCD]----|

        deque.clear();

        // |-----[]-----|
        assert_deque!(deque, 5, [], 5);
    }

    #[test]
    fn clear_zst() {
        let mut deque: LinearDeque<()> = prepare_zst_deque(4);

        deque.clear();

        assert_zst_deque!(deque, 0);
    }

    #[test]
    fn set_reserved_space_keeping() {
        let mut deque = prepare_deque(2, 'A'..='D', 5);
        // |--[ABCD]-----|

        deque.set_reserved_space(SetReservedSpace::Keep, SetReservedSpace::Keep);

        // |--[ABCD]-----|
        assert_deque!(deque, 2, 'A'..='D', 5);
    }

    #[test]
    fn set_reserved_space_growing() {
        let mut deque = prepare_deque(3, 'A'..='D', 1);
        // |---[ABCD]-|

        deque.set_reserved_space(SetReservedSpace::GrowTo(6), SetReservedSpace::GrowTo(2));

        // |------[ABCD]--|
        assert_deque!(deque, 6, 'A'..='D', 2);
    }

    #[test]
    fn set_reserved_space_not_growing() {
        let mut deque = prepare_deque(3, 'A'..='D', 1);
        // |---[ABCD]-|

        deque.set_reserved_space(SetReservedSpace::GrowTo(2), SetReservedSpace::GrowTo(1));

        // |---[ABCD]-|
        assert_deque!(deque, 3, 'A'..='D', 1);
    }

    #[test]
    fn set_reserved_space_shrinking() {
        let mut deque = prepare_deque(5, 'A'..='D', 2);
        // |-----[ABCD]--|

        deque.set_reserved_space(SetReservedSpace::ShrinkTo(2), SetReservedSpace::ShrinkTo(1));

        // |--[ABCD]-|
        assert_deque!(deque, 2, 'A'..='D', 1);
    }

    #[test]
    fn set_reserved_space_not_shrinking() {
        let mut deque = prepare_deque(5, 'A'..='D', 2);
        // |-----[ABCD]--|

        deque.set_reserved_space(SetReservedSpace::ShrinkTo(6), SetReservedSpace::ShrinkTo(3));

        // |-----[ABCD]--|
        assert_deque!(deque, 5, 'A'..='D', 2);
    }

    #[test]
    fn set_reserved_space_exactly_growing() {
        let mut deque = prepare_deque(2, 'A'..='D', 1);
        // |--[ABCD]-|

        deque.set_reserved_space(SetReservedSpace::Exact(5), SetReservedSpace::Exact(2));

        // |-----[ABCD]--|
        assert_deque!(deque, 5, 'A'..='D', 2);
    }

    #[test]
    fn set_reserved_space_exactly_shrinking() {
        let mut deque = prepare_deque(5, 'A'..='D', 2);
        // |-----[ABCD]--|

        deque.set_reserved_space(SetReservedSpace::Exact(2), SetReservedSpace::Exact(1));

        // |--[ABCD]-|
        assert_deque!(deque, 2, 'A'..='D', 1);
    }

    #[test]
    fn set_reserved_space_zst() {
        let mut deque: LinearDeque<()> = prepare_zst_deque(5);

        deque.set_reserved_space(SetReservedSpace::Exact(3), SetReservedSpace::Exact(4));

        assert_zst_deque!(deque, 5);
    }

    #[test]
    fn eq() {
        let mut array = ['A', 'B'];
        let mut array_x = ['B', 'A'];

        let deque = prepare_deque(1, array, 5);

        {
            let slice: &[_] = &array;
            let slice_x: &[_] = &array_x;

            assert!(deque == slice);
            assert!(deque != slice_x);
        }

        assert!(deque == &array);
        assert!(deque != &array_x);

        {
            let slice_mut: &mut [_] = &mut array;
            let slice_mut_x: &mut [_] = &mut array_x;

            assert!(deque == slice_mut);
            assert!(deque != slice_mut_x);
        }

        assert!(deque == &mut array);
        assert!(deque != &mut array_x);

        assert!(deque == array);
        assert!(deque != array_x);

        assert!(deque == Vec::from(array));
        assert!(deque != Vec::from(array_x));

        assert!(deque == prepare_deque(5, array, 0));
        assert!(deque != prepare_deque(1, array_x, 5));
    }

    #[test]
    fn hash() {
        let deque1 = prepare_deque(3, ['A', 'B', 'C'], 5);
        let deque2 = {
            let mut d = LinearDeque::new();
            d.push_back('B');
            d.push_front('A');
            d.push_back('C');
            d
        };
        let mut hasher1 = DefaultHasher::new();
        let mut hasher2 = DefaultHasher::new();

        deque1.hash(&mut hasher1);
        deque2.hash(&mut hasher2);

        assert_eq!(hasher1.finish(), hasher2.finish());
    }

    #[test]
    fn from_iter() {
        let deque = LinearDeque::from_iter('A'..='D');

        assert_deque!(deque, 0, ['A', 'B', 'C', 'D'], 0);
    }

    #[test]
    fn from_iter_zst() {
        let deque = LinearDeque::from_iter(std::iter::repeat(()).take(4));

        assert_zst_deque!(deque, 4);
    }
}
