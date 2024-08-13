`steady_vec`
===============

[`SteadyVec`] is a growable "`Vec`-like" datastructure that does not *move*
elements on *resize*. It maintains the existing allocation whilst using new
allocations for subsequent items.

Like [`Vec`], it begins life without any allocation, then uses a
capacity-doubling strategy each time it runs out of space. It supports many of
the same methods: `push`, `pop`, `get`, `swap`, `insert`, `remove`, `iter`, &c.

### Architecture

The `SteadyVec` stores a `usize` length, and 31 pointers to each of the
optional allocations. If all 31 allocations are used the total allocated
capacity is 2³².

Each individual allocation has a fixed size based on its position;

|**alloc**|`0`|`1`|`2`|`3`|`4`|`5`|`6`|`7`|...|`30`|
|---------|---|---|---|---|---|---|---|---|---|----|
| **size**| 4 | 4 | 8 | 16| 32| 64|128|256|...| 2³¹|

![diagram](diagram.svg)

### why?

When a rust `Vec` becomes full, it *resizes*, which is a process of allocating
a new vector with twice the capacity and then *moving* every element from the
original vector into the new vector. Sometimes you need a growable
"vector-like" thing, but you also need elements not to *move* on growth.

Trade-offs:
- It is not possible to get slices over arbitrary ranges, as the underlying
  elements may not be contiguous in memory.
- `SteadyVec<T>`'s stack-size is large (~256 bytes on 64 bit architectures), so
  stack moves are more expensive. You can use a `Box<SteadyVec<T>>` to mitigate
  this, but that requires an extra indirection for every access.

Since `SteadyVec` guarantees that elements will not *move* when growing (or
shrinking), it may be a useful primitive in the design of certain
datastructures that provide concurrent access.


Future work
-----------

Much of the API surface area of `Vec` has been replicated for `SteadyVec`, but
there are still outstanding methods & traits including:

- binary_search
- sort


License
-------

This crate is licensed under any of the
[Apache license, Version 2.0](./LICENSE-APACHE),
or the
[MIT license](./LICENSE-MIT),
or the
[Zlib license](./LICENSE-ZLIB)
at your option.

Unless explicitly stated otherwise, any contributions you intentionally submit
for inclusion in this work shall be licensed accordingly.
