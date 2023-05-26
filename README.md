# A double-ended queue that can be sliced at any time without preparation.

## [`LinearDeque`] vs [`VecDeque`]

The standard [`VecDeque`] uses a ring buffer. It requires that the
[`make_contiguous`] method is called to ensure that the deque content can
all be referenced in a single slice. [`make_contiguous`] is only callable on
a mutable instance of the deque.

The [`LinearDeque`] provided by this lib uses a linear buffer, keeping all
its content contiguous and allowing to have a slice with all the content at
any time, even when the deque is not mutable.

[`LinearDeque`]: https://docs.rs/linear-deque-rs/0.1.0/linear_deque/struct.LinearDeque.html
[`VecDeque`]: https://doc.rust-lang.org/std/collections/struct.VecDeque.html
[`make_contiguous`]: https://doc.rust-lang.org/std/collections/struct.VecDeque.html#method.make_contiguous
