#![doc = include_str!("../README.md")]

pub mod iter;
mod manual_heap_array_vec;
#[cfg(any(test, doctest))]
mod tests;

use {
  self::{
    iter::{
      BoxedSteadyVecIntoIter, SteadyVecIntoIter, SteadyVecIter,
      SteadyVecIterMut,
    },
    manual_heap_array_vec::ManualHeapArrayVec,
  },
  ::core::{
    iter::zip,
    mem::MaybeUninit,
    ops::{Index, IndexMut},
    ptr,
  },
};

/// A heap allocated indexable array-like datastructure, that will grow without
/// moving existing elements
pub struct SteadyVec<E> {
  /// There are 32 "sub-arrays", where each successive subarray is double the
  /// size of the previous. The first 2 subarrays have a capacity of 2; this
  /// allows for a maximum limit of 2³² elements to be stored.
  subarrays: [Option<ManualHeapArrayVec<E>>; 32],
  /// Items from 0..len are initialised, but items from len.. are uninit or
  /// the subarrays may be `None`.
  len: usize,
}

// There's a somewhat large amount of unsafe code here. The safety conditions
// are noted in each case, but almost all of it relies on a couple of simple
// invariants:
//
// - `len` delimits the number of initialised elements.
//
// - each subarray has a fixed capacity based on it's position, given by the
//   `subarray_capacity` function.
//
// - Elements are counted starting from the 0th subarray; filling every "slot"
//   in that subarray, in order; then moving to the 1st subarray, and so on.
//
//   The coordinates for the element at a particular index is given by the
//    function `index_metadata`. Alternatively the total range of indices
//   corresponding to a particular subarray is given by the function
//   `subarray_index_range`.

struct IndexMetadata {
  /// subarray number
  subarray_n: usize,
  /// subarray element (index into the subarray)
  element: usize,
}

/// The size of subarray number `n`
///
/// Counting from `n=0`, follows the pattern:
/// `2`, `2`, `4`, `8`, `16`, `32`, ..
#[inline]
fn subarray_capacity(n: usize) -> usize {
  // The very first subarray needs special handling, because it has a capacity
  // of 2, instead of 1. We use `max` for this.
  (1 << n).max(2)
}

/// The range of indices (inclusive) corresponding to subarray number `n`
///
/// Counting from `n=0`, follows the pattern:
/// `(0, 1)`, `(2, 3)`, `(4, 7)`, `(8, 15)`, `(16, 31)`..
#[inline]
fn subarray_index_range(n: usize) -> (usize, usize) {
  // The very first subarray needs special handling, because its first index is
  // 0 instead of 1. We mask off the 1st bit for this.
  let first = (1 << n) & (!0b1);
  let last = (1 << (n + 1)) - 1;
  (first, last)
}

/// Takes an `index` and returns the corresponding subarray number, `n`
///
/// This is effectively the inverse of `subarray_index_range`.
///
/// Follows the pattern:
/// `0..=1` -> `0`
/// `2..=3` -> `1`
/// `4..=7` -> `2`
/// `8..=15` -> `3`
/// `16..=31` -> `4`
#[inline]
fn index_to_subarray_n(index: usize) -> usize {
  // The very first subarray needs special handling, because an index of 0
  // corresponds to subarray 0, but log2(0) is undefined.
  (index.max(1)).ilog2() as usize
}

/// Takes an index and returns the corresponding subarray number and the index
/// of the element within that subarray
///
/// index should be in the range `0..SteadyVec::MAX_LEN`
#[inline]
fn index_metadata(index: usize) -> IndexMetadata {
  let subarray_n = index_to_subarray_n(index);
  let (first_index, _) = subarray_index_range(subarray_n);
  let element = index - first_index;

  IndexMetadata {
    subarray_n,
    element,
  }
}

impl<E> SteadyVec<E> {
  /// The maximum capacity of a steady vec, 2³²
  pub const MAX_CAPACITY: usize = u32::MAX as usize + 1;

  /// Constructs a new, empty `Box<SteadyVec<T>>`
  ///
  /// Will not allocate subarrays until elements are pushed.
  ///
  /// Note: `Box<SteadyVec>` imposes an extra indirection on every access, but
  /// stack moves are cheaper compared to a bare [`SteadyVec`](SteadyVec::new).
  pub fn new_boxed() -> Box<Self> {
    Box::new(SteadyVec {
      subarrays: [ManualHeapArrayVec::OPTION_NONE; 32],
      len: 0,
    })
  }

  /// Constructs a new, empty `SteadyVec<T>`
  ///
  /// Will not allocate until elements are pushed.
  ///
  /// Note: `SteadyVec` is a reasonably large type to have on the stack (264
  /// bytes), so you may prefer [`Box<SteadyVec>`](SteadyVec::new_boxed) which
  /// stores the subarray on the heap instead. The trade-off is that the Box
  /// imposes an extra indirection on accesses, but stack-moves are cheaper.
  pub const fn new() -> Self {
    SteadyVec {
      subarrays: [ManualHeapArrayVec::OPTION_NONE; 32],
      len: 0,
    }
  }

  /// Returns the number of elements in the `SteadyVec`
  pub fn len(&self) -> usize {
    self.len
  }

  /// Returns `true` if the `SteadyVec` contains no elements
  pub fn is_empty(&self) -> bool {
    self.len == 0
  }

  /// Returns the total number of elements the `SteadyVec` can hold without
  /// allocating
  pub fn capacity(&self) -> usize {
    let last_subarray_n = self.subarrays.iter().flatten().count() - 1;
    subarray_capacity(last_subarray_n) * 2
  }

  /// Reserves capacity for at least `additional` more elements
  ///
  /// After calling `reserve`, the capacity will be greater than or equal to
  /// `self.len() + additional`.
  ///
  /// # Panics
  ///
  /// Panics if the new capacity would exceed [`Self::MAX_CAPACITY`].
  pub fn reserve(&mut self, additional: usize) {
    let new_min_capacity = self.len + additional;
    if new_min_capacity > Self::MAX_CAPACITY {
      panic!(
        "capacity: {new_min_capacity} would exceed maximum: {max_capacity}",
        max_capacity = Self::MAX_CAPACITY
      );
    }

    let required_subarray_n = index_to_subarray_n(new_min_capacity - 1);
    let last_subarray_n = index_to_subarray_n(self.len - 1);
    for subarray_n in (last_subarray_n + 1)..=required_subarray_n {
      if self.subarrays[subarray_n].is_none() {
        self.subarrays[subarray_n] =
          Some(ManualHeapArrayVec::new(subarray_capacity(subarray_n)));
      }
    }
  }

  /// Clears the `SteadyVec`, dropping all values
  ///
  /// Does not change the allocated capacity.
  pub fn clear(&mut self) {
    self.truncate(0)
  }

  /// Shortens the `SteadyVec`, keeping the first `len` elements and dropping
  /// the remaining
  ///
  /// If `len` is greater than or equal to the current length, this does
  /// nothing.
  pub fn truncate(&mut self, len: usize) {
    if len >= self.len || self.is_empty() {
      return;
    }

    // first and last indices to remove (inclusive)
    let first_index_meta = index_metadata(len);
    let last_index_meta = index_metadata(self.len - 1);

    // safety:
    // - Similarly to `Vec::clear`, setting `self.len` before calling
    //   `drop_in_place` allows leaking the elements if a `Drop` impl panics,
    //   rather than calling the destructor for some elements twice.
    self.len = len;

    for n in first_index_meta.subarray_n..=last_index_meta.subarray_n {
      let first_element = if n == first_index_meta.subarray_n {
        first_index_meta.element
      } else {
        0
      };
      let last_element = if n == last_index_meta.subarray_n {
        last_index_meta.element
      } else {
        subarray_capacity(n) - 1
      };
      assert!(first_element <= last_element);
      // safety: the value of `self.len` promises
      // - the subarray exists,
      // - the elements from 0..len are initialised;
      //   first_element..=last_elements is a subset.
      unsafe {
        let subarray = self.subarrays[n].as_mut().unwrap_unchecked();
        subarray.drop_in_place(first_element, last_element);
      }
    }
  }

  /// Push a new element onto the end
  ///
  /// # Panics
  ///
  /// Panics if the new length would exceed [`Self::MAX_CAPACITY`].
  pub fn push(&mut self, value: E) {
    assert!(
      self.len < Self::MAX_CAPACITY,
      "capacity: {new_capacity} would exceed maximum: {max_capacity}",
      new_capacity = self.len,
      max_capacity = Self::MAX_CAPACITY
    );

    let index_metadata = index_metadata(self.len);

    // may need to allocate a new subarray if subarray is None
    let subarray = self.subarrays[index_metadata.subarray_n]
      .get_or_insert_with(|| {
        ManualHeapArrayVec::new(subarray_capacity(index_metadata.subarray_n))
      });

    // safety: by construction `index_metadata.element` is a valid element
    // index for the subarray.
    unsafe { subarray.set_with(index_metadata.element, || value) };
    self.len += 1;
  }

  /// Remove the last element and return it, or return `None` if empty
  pub fn pop(&mut self) -> Option<E> {
    if self.is_empty() {
      return None;
    }

    // safety:
    // - `len` != 0
    // - the value of `len` promises that subarray & element exist.
    // - `len` is decremented, so further calls will not take a value from the
    //   now unitialised memory.
    self.len -= 1;
    let index_metadata = index_metadata(self.len);
    let element = unsafe {
      let subarray = self.subarrays[index_metadata.subarray_n]
        .as_mut()
        .unwrap_unchecked();

      subarray.take_element(index_metadata.element)
    };

    Some(element)
  }

  /// Get the element at the index
  pub fn get(&self, index: usize) -> Option<&E> {
    if index >= self.len {
      return None;
    }

    let index_metadata = index_metadata(index);

    // safety: the value of `self.len` tells us
    // - the subarray exists, and
    // - item at `index` exists and is initialised within that subarray.
    let element = unsafe {
      let subarray = self.subarrays[index_metadata.subarray_n]
        .as_ref()
        .unwrap_unchecked();

      subarray
        .as_slice(index_metadata.element, index_metadata.element)
        .get_unchecked(0)
    };

    Some(element)
  }

  /// Mutably get the element at the index
  pub fn get_mut(&mut self, index: usize) -> Option<&mut E> {
    if index >= self.len {
      return None;
    }

    let index_metadata = index_metadata(index);

    // safety: the value of `self.len` tells us
    // - the subarray exists, and
    // - item at `index` exists and is initialised within that subarray.
    let element = unsafe {
      let subarray = self.subarrays[index_metadata.subarray_n]
        .as_mut()
        .unwrap_unchecked();

      subarray
        .as_slice_mut(index_metadata.element, index_metadata.element)
        .get_unchecked_mut(0)
    };

    Some(element)
  }

  /// Insert an element at `index`, shifting all following elements to the
  /// right
  ///
  /// O(n) time complexity.
  ///
  /// # Panics
  ///
  /// - Panics if `index` is greater than the length.
  /// - Panics if the new capacity would exceed [`Self::MAX_CAPACITY`].
  pub fn insert(&mut self, index: usize, value: E) {
    // index out of bounds
    if index > self.len {
      panic!(
        "index is out of bounds, index: {index}, len: {len}",
        len = self.len
      );
    }
    self.reserve(1);

    // note: this is the value last_index, including the extra element about to
    // be inserted
    let last_index = self.len;

    let first_subarray_n = index_to_subarray_n(index);
    let last_subarray_n = index_to_subarray_n(last_index);

    let mut temp = MaybeUninit::new(value);
    for (subarray_n, subarray_cap) in
      (first_subarray_n..=last_subarray_n).map(|n| (n, subarray_capacity(n)))
    {
      // safety:
      // - `len` promises that `subarray_n` exists.
      let subarray =
        unsafe { self.subarrays[subarray_n].as_mut().unwrap_unchecked() };

      let first_element = if subarray_n == first_subarray_n {
        index_metadata(index).element
      } else {
        0
      };
      let last_element = if subarray_n == last_subarray_n {
        index_metadata(last_index).element
      } else {
        subarray_cap - 1
      };

      // read the last element value into `last`, then shift all the previous
      // elements right one space using an overlapping copy.
      unsafe {
        // safety:
        // - `len` promises that `first_element` & `last_element` exist (they
        //   may be equal).
        let slice: &mut [MaybeUninit<E>] =
          subarray.as_uninit_slice_mut(first_element, last_element);
        // safety:
        // - `len` promises that `last` will be initialised, except in the last
        //   iteration.
        let last: MaybeUninit<E> = {
          let ptr: &MaybeUninit<E> =
            slice.get_unchecked(last_element - first_element);
          ptr::read(ptr)
        };

        // safety:
        // - the slice contains at least 1 element (`last`).
        // - the pointers are valid, as noted above.
        let slice_mut_ptr = slice.as_mut_ptr();
        ptr::copy(slice_mut_ptr, slice_mut_ptr.add(1), slice.len() - 1);

        // write the `temp` value into the newly created space at the start.
        // safety:
        // - the slice contains at least 1 element, so `first` exists
        // - temp is always initialised, except at the very end of the last
        //   iteration. After the loop exits, temp is never used again
        let first: &mut MaybeUninit<E> = slice.get_unchecked_mut(0);
        first.write(temp.assume_init());

        // shift the new `last` into `temp`
        temp = last;
      }
    }

    self.len += 1;
  }

  /// Remove and return the element at `index`, shifting all following elements
  /// to the left
  ///
  /// O(n) time complexity.
  ///
  /// # Panics
  ///
  ///  Panics if `index` is greater than or equal to the length.
  pub fn remove(&mut self, index: usize) -> E {
    // index out of bounds; or empty
    if index >= self.len {
      panic!(
        "index is out of bounds, index: {index}, len: {len}",
        len = self.len
      );
    }

    self.len -= 1;
    let last_index = self.len;
    let first_subarray_n = index_to_subarray_n(index);
    let last_subarray_n = index_to_subarray_n(last_index);

    // Iterate backwards through the subarrays containing initialised elements.
    let mut temp = MaybeUninit::uninit();
    for (subarray_n, subarray_cap) in (first_subarray_n..=last_subarray_n)
      .rev()
      .map(|n| (n, subarray_capacity(n)))
    {
      // safety:
      // - `len` promises that `subarray_n` exists.
      let subarray =
        unsafe { self.subarrays[subarray_n].as_mut().unwrap_unchecked() };

      let first_element = if subarray_n == first_subarray_n {
        index_metadata(index).element
      } else {
        0
      };
      let last_element = if subarray_n == last_subarray_n {
        index_metadata(last_index).element
      } else {
        subarray_cap - 1
      };

      // read the first element value into `first`, then shift all the
      // following elements left one space using an overlapping copy.
      unsafe {
        // safety:
        // - `len` promises that `first_element` & `last_element` exist (they
        //   may be equal).
        let first = subarray.take_element(first_element);
        let slice: &mut [MaybeUninit<E>] =
          subarray.as_uninit_slice_mut(first_element, last_element);

        // safety:
        // - the slice contains at least 1 element (`first`).
        // - the pointers are valid, as noted above.
        let slice_mut_ptr = slice.as_mut_ptr();
        ptr::copy(slice_mut_ptr.add(1), slice_mut_ptr, slice.len() - 1);

        // write the `temp` value into the newly created space at the end.
        // safety:
        // - the slice contains at least 1 element, so `last` exists.
        // - if this is the last subarray, temp is uninitialised. Writing
        //   `temp` to `last` is still okay, because we decremented the length
        //   of the `SteadyVec` - which makes `last` inaccessible.
        let last: &mut MaybeUninit<E> =
          slice.get_unchecked_mut(slice.len() - 1);
        *last = temp;

        // shift the new `first` into `temp`
        temp = MaybeUninit::new(first);
      }
    }

    // safety:
    // - temp will be init, as long as the for-loop above runs at least once,
    //   which is always the case since `len` is known not to be zero due to
    //   the bounds check condition at the top.
    unsafe { temp.assume_init() }
  }

  /// Remove and return the element at `index`, replacing it with the last
  /// element in the `SteadyVec`
  ///
  /// O(1) time complexity.
  ///
  /// # Panics
  ///
  /// Panics if `index` is greater than or equal to the length.
  pub fn swap_remove(&mut self, index: usize) -> E {
    // index out of bounds; or empty
    if index >= self.len {
      panic!(
        "index is out of bounds, index: {index}, len: {len}",
        len = self.len
      );
    } else {
      self.len -= 1;
      let last_index = self.len;

      let value;
      unsafe {
        // safety:
        // - `len` promises that the subarray for, and that the element at
        //   `last_index` exists.
        // - `len` is decremented prevents further access to the element at
        //   `last_index`.
        let last_element = {
          let meta = index_metadata(last_index);
          let subarray =
            self.subarrays[meta.subarray_n].as_mut().unwrap_unchecked();

          subarray.take_element(meta.element)
        };
        // safety:
        // - `len` promises that the subarray for, and that the element at
        //   `index` exists.
        // - element at `index` is removed, leaving that slot uninit, but we
        //   then immediately set that slot with the value of `last_element`.
        // - in the case that `index == last_index`, it doesn't matter that the
        //   element is written back into the same place while we also return
        //   it in `value`; we decrement `len` regardless, preventing access to
        //   the duplicate.
        {
          let meta = index_metadata(index);
          let subarray =
            self.subarrays[meta.subarray_n].as_mut().unwrap_unchecked();

          value = subarray.take_element(meta.element);
          subarray.set_with(meta.element, || last_element);
        }
      }

      value
    }
  }

  /// Swap two elements
  ///
  /// # Panics
  ///
  /// Panics if either `a_index` or `b_index` are greater than or equal to the
  /// length.
  pub fn swap(&mut self, a_index: usize, b_index: usize) {
    if a_index >= self.len || b_index >= self.len {
      panic!(
        "index is out of bounds, a_index: {a_index}, b_index: {b_index}, len: {len}",
        len = self.len
      );
    } else {
      unsafe {
        // safety:
        // - `len` promises that the subarray for, and that the element at
        //   `a` exists.
        let a_ptr: *mut E = {
          let meta = index_metadata(a_index);
          let subarray =
            self.subarrays[meta.subarray_n].as_mut().unwrap_unchecked();

          subarray
            .as_slice_mut(meta.element, meta.element)
            .get_unchecked_mut(0)
        };
        // safety:
        // - `len` promises that the subarray for, and that the element at
        //   `b` exists.
        let b_ptr: *mut E = {
          let meta = index_metadata(b_index);
          let subarray =
            self.subarrays[meta.subarray_n].as_mut().unwrap_unchecked();

          subarray
            .as_slice_mut(meta.element, meta.element)
            .get_unchecked_mut(0)
        };

        // safety: `a_ptr` & `b_ptr` are properly aligned and valid as above.
        // also note: `ptr::swap` allows them to overlap
        ptr::swap(a_ptr, b_ptr);
      }
    }
  }

  /// Returns an iterator over each element of the collection
  pub fn iter(&self) -> SteadyVecIter<E> {
    SteadyVecIter::new(self)
  }

  /// Returns an iterator that allows modifying each element of the collection
  pub fn iter_mut(&mut self) -> SteadyVecIterMut<E> {
    SteadyVecIterMut::new(self)
  }

  // pub fn retain(&mut self, f: impl FnMut(&E) -> bool) {
  //   todo!()
  // }

  // pub fn retain_mut(&mut self, f: impl FnMut(&mut E) -> bool) {
  //   todo!()
  // }

  /// Resizes the `SteadyVec` in place
  ///
  /// If `new_len` is less than `len` then the `SteadyVec` is truncated. If
  /// `new_len` is greater than `len` then it is extended by repeating the
  /// supplied value.
  ///
  /// # Panics
  ///
  /// Panics if `new_len` is greater than [`Self::MAX_CAPACITY`].
  pub fn resize(&mut self, new_len: usize, value: E)
  where
    E: Clone,
  {
    self.resize_with(new_len, || value.clone())
  }

  /// Resizes the `SteadyVec` in place
  ///
  /// If `new_len` is less than `len` then the `SteadyVec` is truncated. If
  /// `new_len` is greater than `len` then it is extended using the supplied
  /// closure to generate values.
  ///
  /// # Panics
  ///
  /// Panics if `new_len` is greater than [`Self::MAX_CAPACITY`].
  pub fn resize_with(&mut self, new_len: usize, mut f: impl FnMut() -> E) {
    if new_len <= self.len {
      self.truncate(new_len)
    } else {
      self.reserve(new_len - self.len);
      for index in self.len..new_len {
        let index_meta = index_metadata(index);
        // safety: we called `reserve` to ensure all needed subarrays exist.
        let subarray = unsafe {
          self
            .subarrays
            .get_unchecked_mut(index_meta.subarray_n)
            .as_mut()
            .unwrap_unchecked()
        };

        // safety: by construction `index_metadata.element` is a valid element
        // index for the subarray.
        unsafe { subarray.set_with(index_meta.element, &mut f) };
      }
      self.len = new_len;
    }
  }

  /// Shrinks the capacity of the `SteadyVec` as much as possible
  ///
  /// The resulting `SteadyVec` is still likely to have excess capacity after
  /// calling this.
  pub fn shrink_to_fit(&mut self) {
    self.shrink_to(self.len)
  }

  /// Shrinks the capacity of the `SteadyVec` with a lower bound
  ///
  /// The capacity will remain at least as large as both the length and the
  /// supplied value.
  ///
  /// Does nothing if the supplied value is greater than the existing capacity.
  pub fn shrink_to(&mut self, min_capacity: usize) {
    let first_unneeded = index_to_subarray_n(self.len.min(min_capacity));
    let last_existing = index_to_subarray_n(self.capacity().saturating_sub(1));

    for n in first_unneeded..=last_existing {
      if let Some(subarray) = self.subarrays[n].take() {
        // safety: the capacity is known from its index
        unsafe { subarray.destroy(subarray_capacity(n)) }
      }
    }
  }
}

impl<E> Index<usize> for SteadyVec<E> {
  type Output = E;

  fn index(&self, index: usize) -> &Self::Output {
    self.get(index).expect("index is out of bounds")
  }
}

impl<E> IndexMut<usize> for SteadyVec<E> {
  fn index_mut(&mut self, index: usize) -> &mut Self::Output {
    self.get_mut(index).expect("index is out of bounds")
  }
}

impl<'s, E> IntoIterator for &'s SteadyVec<E> {
  type Item = <SteadyVecIter<'s, E> as Iterator>::Item;
  type IntoIter = SteadyVecIter<'s, E>;

  /// Returns an iterator over each element of the collection
  fn into_iter(self) -> SteadyVecIter<'s, E> {
    self.iter()
  }
}

impl<'s, E> IntoIterator for &'s mut SteadyVec<E> {
  type Item = <SteadyVecIterMut<'s, E> as Iterator>::Item;
  type IntoIter = SteadyVecIterMut<'s, E>;

  /// Returns an iterator that allows modifying each element of the collection
  fn into_iter(self) -> SteadyVecIterMut<'s, E> {
    self.iter_mut()
  }
}

impl<E> IntoIterator for SteadyVec<E> {
  type Item = <SteadyVecIntoIter<E> as Iterator>::Item;
  type IntoIter = SteadyVecIntoIter<E>;

  /// Returns an iterator that moves each value out of the `SteadyVec` (from
  /// start to end)
  ///
  /// The SteadyVec cannot be used after calling this.
  fn into_iter(self) -> SteadyVecIntoIter<E> {
    SteadyVecIntoIter::new(self)
  }
}

impl<E> IntoIterator for Box<SteadyVec<E>> {
  type Item = <BoxedSteadyVecIntoIter<E> as Iterator>::Item;
  type IntoIter = BoxedSteadyVecIntoIter<E>;

  /// Returns an iterator that moves each value out of the `SteadyVec` (from
  /// start to end)
  ///
  /// The SteadyVec cannot be used after calling this.
  fn into_iter(self) -> BoxedSteadyVecIntoIter<E> {
    BoxedSteadyVecIntoIter::new(self)
  }
}

impl<E> Extend<E> for SteadyVec<E> {
  fn extend<I: IntoIterator<Item = E>>(&mut self, iter: I) {
    for item in iter {
      self.push(item)
    }
  }
}

impl<E> FromIterator<E> for SteadyVec<E> {
  fn from_iter<I: IntoIterator<Item = E>>(iter: I) -> Self {
    let mut steady_vec = SteadyVec::new();
    for item in iter {
      steady_vec.push(item)
    }
    steady_vec
  }
}

impl<E> FromIterator<E> for Box<SteadyVec<E>> {
  fn from_iter<I: IntoIterator<Item = E>>(iter: I) -> Self {
    let mut steady_vec = SteadyVec::new_boxed();
    for item in iter {
      steady_vec.push(item)
    }
    steady_vec
  }
}

impl<E> Clone for SteadyVec<E>
where
  E: Clone,
{
  /// Returns a copy of the SteadyVec
  ///
  /// Only allocates as much as is needed to store the elements, so the
  /// capacity of the new SteadyVec may not match the capacity of the source.
  fn clone(&self) -> Self {
    let mut dest = SteadyVec::new();
    dest.clone_from(self);
    dest
  }

  /// Copy assignment of `source` into `self`
  ///
  /// Reuses allocation in `self` where possible, otherwise only allocates as
  /// much as is needed. This means the capacity of the result may not match
  /// the capacity of the source.
  fn clone_from(&mut self, source: &Self) {
    self.clear();

    if source.is_empty() {
      return;
    }

    let last_index_meta = index_metadata(source.len - 1);
    for subarray_n in 0..=last_index_meta.subarray_n {
      let subarray_capacity = subarray_capacity(subarray_n);

      // use the existing allocation, if it exists
      let dst_subarray = self.subarrays[subarray_n]
        .get_or_insert_with(|| ManualHeapArrayVec::new(subarray_capacity));

      // safety:
      // for src_subarray_slice, `source.len` indicates
      // - subarray_n exists
      // - elements from 0..=last_element are initialised
      // for dst_subarray_slice
      // - new_subarray is known to have capacity > last_element
      let last_element = if subarray_n == last_index_meta.subarray_n {
        last_index_meta.element
      } else {
        subarray_capacity - 1
      };
      let dst_subarray_slice =
        unsafe { dst_subarray.as_uninit_slice_mut(0, last_element) };

      let src_subarray_slice = unsafe {
        let src_subarray =
          source.subarrays[subarray_n].as_ref().unwrap_unchecked();
        src_subarray.as_slice(0, last_element)
      };

      // I'm told writing items individually should compile favorably, but
      // could use `MaybeUninit::write_slice_cloned` when stabilised
      // tracking issue: https://github.com/rust-lang/rust/issues/79995
      //
      // An alternative would be to temporarily turn both slices into
      // `[ManuallyDrop<T>]` to facilitate the copy whilst preventing Drop
      // being called on the Uninit memory.
      for (dst, clone) in zip(
        dst_subarray_slice.iter_mut(),
        src_subarray_slice.iter().cloned(),
      ) {
        dst.write(clone);
      }
    }

    // We set the len at the end. This way, if a panic happens in the earlier
    // code (in a Clone implementation or something), then the new SteadyVec's
    // Drop implementation should not try to Drop the elements which might not
    // be initialised.
    self.len = source.len;
  }
}

impl<E> Drop for SteadyVec<E> {
  fn drop(&mut self) {
    // drop-in-place all the elements
    self.clear();
    // drop the allocations
    for (subarray_n, subarray) in self.subarrays.iter_mut().enumerate() {
      if let Some(subarray) = subarray.take() {
        // safety: the capacity for every subarray is known.
        unsafe {
          subarray.destroy(subarray_capacity(subarray_n));
        }
      }
    }
  }
}
