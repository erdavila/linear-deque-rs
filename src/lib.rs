use std::ptr::NonNull;

pub struct LinearDeque<T> {
    ptr: NonNull<T>,
    cap: usize,
    len: usize,
    off: usize,
}
