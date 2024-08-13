//! Iterator implementations

use {
  super::{index_metadata, subarray_capacity, SteadyVec},
  ::core::{
    cmp::Ordering, iter::FusedIterator, marker::PhantomData, mem::ManuallyDrop,
  },
};

/// A borrowing Iterator
///
/// An iterator that borrows each value of the `SteadyVec` (from start to end).
/// Created using [`SteadyVec::iter`].
///
/// This iterator also implements [`FusedIterator`], [`ExactSizeIterator`], &
/// [`DoubleEndedIterator`].
pub struct SteadyVecIter<'s, E: 's> {
  steady_vec: &'s SteadyVec<E>,
  index: usize,
  len: usize,
}

impl<'s, E> SteadyVecIter<'s, E> {
  pub(crate) fn new(steady_vec: &'s SteadyVec<E>) -> Self {
    SteadyVecIter {
      index: 0,
      len: steady_vec.len,
      steady_vec,
    }
  }
}

impl<'s, E> Iterator for SteadyVecIter<'s, E> {
  type Item = &'s E;

  fn next(&mut self) -> Option<Self::Item> {
    let element = self.steady_vec.get(self.index);
    if element.is_some() {
      self.index += 1;
    }
    element
  }

  fn size_hint(&self) -> (usize, Option<usize>) {
    let remaining = self.len - self.index;
    (remaining, Some(remaining))
  }
}

impl<'s, E> FusedIterator for SteadyVecIter<'s, E> {}

impl<'s, E> ExactSizeIterator for SteadyVecIter<'s, E> {
  fn len(&self) -> usize {
    let (lower, _) = self.size_hint();
    lower
  }
}

impl<'s, E> DoubleEndedIterator for SteadyVecIter<'s, E> {
  fn next_back(&mut self) -> Option<Self::Item> {
    let element = self.steady_vec.get(self.len - 1);
    if element.is_some() {
      self.len -= 1;
    }
    element
  }
}

/// A mutably borrowing Iterator
///
/// An iterator that mutably borrows each value of the `SteadyVec` (from start
/// to end). Created using [`SteadyVec::iter_mut`].
///
/// This iterator also implements [`FusedIterator`], [`ExactSizeIterator`], &
/// [`DoubleEndedIterator`].
pub struct SteadyVecIterMut<'s, E: 's> {
  steady_vec: *mut SteadyVec<E>,
  index: usize,
  len: usize,
  _lifetime: PhantomData<&'s mut SteadyVec<E>>,
}

impl<'s, E: 's> SteadyVecIterMut<'s, E> {
  pub(crate) fn new(steady_vec: &mut SteadyVec<E>) -> Self {
    SteadyVecIterMut {
      index: 0,
      len: steady_vec.len,
      steady_vec,
      _lifetime: PhantomData,
    }
  }
}

impl<'s, E> Iterator for SteadyVecIterMut<'s, E> {
  type Item = &'s mut E;

  fn next(&mut self) -> Option<Self::Item> {
    // safety:
    // - the lifetime of the pointer to steady_vec is known to be alive since
    //   the iterator also explicitly stores it.
    // - the ptr was known to be non-null when the iterator was constructed.
    let steady_vec: &'s mut SteadyVec<E> =
      unsafe { self.steady_vec.as_mut().unwrap_unchecked() };

    let element = steady_vec.get_mut(self.index);
    if element.is_some() {
      self.index += 1;
    }
    element
  }

  fn size_hint(&self) -> (usize, Option<usize>) {
    let remaining = self.len - self.index;
    (remaining, Some(remaining))
  }
}

impl<'s, E> ExactSizeIterator for SteadyVecIterMut<'s, E> {
  fn len(&self) -> usize {
    let (lower, _) = self.size_hint();
    lower
  }
}

impl<'s, E> FusedIterator for SteadyVecIterMut<'s, E> {}

impl<'s, E> DoubleEndedIterator for SteadyVecIterMut<'s, E> {
  fn next_back(&mut self) -> Option<Self::Item> {
    // safety:
    // - the lifetime of the pointer to steady_vec is known to be alive since
    //   the iterator also explicitly captures the lifetime of an exclusive
    //   reference to the underlying SteadyVec.
    // - the ptr was known to be non-null when the iterator was constructed.
    let steady_vec: &'s mut SteadyVec<E> =
      unsafe { self.steady_vec.as_mut().unwrap_unchecked() };

    let element = steady_vec.get_mut(self.len - 1);
    if element.is_some() {
      self.len -= 1;
    }
    element
  }
}

/// A consuming Iterator
///
/// An iterator that moves each value out of the `SteadyVec` (from start to
/// end). The SteadyVec cannot be used after calling this. Created using
/// [`SteadyVec::into_iter`].
///
/// `SteadyVecIntoIter` differs to [`BoxedSteadyVecIntoIter`] in that the
/// source `SteadyVec` is stored inside the iterator object on the stack,
/// increasing the size of the iterator object on the stack.
///
/// This iterator also implements [`FusedIterator`], [`ExactSizeIterator`], &
/// [`DoubleEndedIterator`].
//
// # Safety
//
// We are breaking the normal invariants of SteadyVec here, since we are moving
// elements out of the SteadyVec, effectivly popping from the front.
//
// This means that not all the elements from 0..len may not be initialised.
// Instead we explicitly manage the bounds of the initialised memory.
//
// Unfortunately this also means we must manually drop the underlying
// SteadyVec; Only the memory from self.index..=self.len is known to be
// *initialised*.
pub struct SteadyVecIntoIter<E> {
  steady_vec: ManuallyDrop<SteadyVec<E>>,
  // next index to read
  next: usize,
  // the last index to read + 1 (exclusive)
  end: usize,
}

impl<E> SteadyVecIntoIter<E> {
  pub(crate) fn new(steady_vec: SteadyVec<E>) -> Self {
    SteadyVecIntoIter {
      next: 0,
      end: steady_vec.len,
      steady_vec: ManuallyDrop::new(steady_vec),
    }
  }
}

/// A consuming Iterator
///
/// An iterator that moves each value out of the `SteadyVec` (from start to
/// end). The SteadyVec cannot be used after calling this. Created using
/// [`SteadyVec::into_iter`].
///
/// `BoxedSteadyVecIntoIter` differs to [`SteadyVecIntoIter`] in that the
/// source `SteadyVec` is stored on the heap, reducing the size of the
/// iterator object on the stack.
///
/// This iterator also implements [`FusedIterator`], [`ExactSizeIterator`], &
/// [`DoubleEndedIterator`].
pub struct BoxedSteadyVecIntoIter<E> {
  steady_vec: Box<ManuallyDrop<SteadyVec<E>>>,
  // next index to read
  next: usize,
  // the last index to read + 1 (exclusive)
  end: usize,
}

impl<E> BoxedSteadyVecIntoIter<E> {
  pub(crate) fn new(
    steady_vec: Box<SteadyVec<E>>,
  ) -> BoxedSteadyVecIntoIter<E> {
    // We want to manually drop the SteadyVec, but we also want the box to be
    // freed when appropriate, so we create the ManuallyDrop in-place.
    // safety: `ManuallyDrop<SteadyVec>` has the same layout as `SteadyVec`
    let steady_vec = unsafe {
      ::core::mem::transmute::<
        Box<SteadyVec<E>>,
        Box<ManuallyDrop<SteadyVec<E>>>,
      >(steady_vec)
    };

    BoxedSteadyVecIntoIter {
      next: 0,
      end: steady_vec.len,
      steady_vec,
    }
  }
}

macro_rules! impl_steady_vec_into_iter {
  ($steady_vec_variant:ident) => {
    impl<E> Iterator for $steady_vec_variant<E> {
      type Item = E;

      fn next(&mut self) -> Option<E> {
        if self.next >= self.end {
          return None;
        }

        let index_metadata = index_metadata(self.next);
        // safety:
        // - the value of `self.len` tells us the subarray exists.
        // - the value of `self.index` tells us the item at that index is
        //   initialised.
        // - after this, the memory from 0..=self.index is *uninitialised*.
        //   (see the note about ManuallyDrop above)
        let element = unsafe {
          let subarray = self.steady_vec.subarrays[index_metadata.subarray_n]
            .as_mut()
            .unwrap_unchecked();

          subarray.take_element(index_metadata.element)
        };

        self.next += 1;

        Some(element)
      }

      fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.end - self.next;
        (remaining, Some(remaining))
      }
    }

    impl<E> FusedIterator for $steady_vec_variant<E> {}

    impl<E> ExactSizeIterator for $steady_vec_variant<E> {
      fn len(&self) -> usize {
        let (lower, _) = self.size_hint();
        lower
      }
    }

    impl<E> DoubleEndedIterator for $steady_vec_variant<E> {
      fn next_back(&mut self) -> Option<E> {
        if self.next >= self.end {
          return None;
        }

        self.end -= 1;

        let index_metadata = index_metadata(self.end);
        // safety:
        // - the value of `self.len` tells us the subarray exists & (guarded by the
        //   comparison above) that the item at the index self.len-1 is
        //   initialised.
        // - after this, the memory at self.len-1 is *uninitialised*.
        //   (see the note about ManuallyDrop above)
        let element = unsafe {
          let subarray = self.steady_vec.subarrays[index_metadata.subarray_n]
            .as_mut()
            .unwrap_unchecked();

          subarray.take_element(index_metadata.element)
        };

        Some(element)
      }
    }

    impl<E> Drop for $steady_vec_variant<E> {
      fn drop(&mut self) {
        if self.len() != 0 {
          // note: see the safety conditions noted above (on `SteadyVecIntoIter`)
          // which must be met in this drop implementation.
          let first_index_metadata = index_metadata(self.next);
          let first_subarray_n = first_index_metadata.subarray_n;
          let last_index_metadata = index_metadata(self.end - 1);
          let last_subarray_n = last_index_metadata.subarray_n;

          for (subarray_n, subarray) in
            self.steady_vec.subarrays.iter_mut().enumerate()
          {
            if let Some(mut subarray) = subarray.take() {
              let subarray_capacity = subarray_capacity(subarray_n);

              let drop_start = match first_subarray_n.cmp(&subarray_n) {
                Ordering::Less => Some(0),
                Ordering::Equal => Some(first_index_metadata.element),
                Ordering::Greater => None,
              };
              let drop_end = match subarray_n.cmp(&last_subarray_n) {
                Ordering::Less => Some(subarray_capacity - 1),
                Ordering::Equal => Some(last_index_metadata.element),
                Ordering::Greater => None,
              };

              // safety:
              // - we compute the bounds of the initialised elements to be
              //   dropped using the index & len.
              // - subarray capacity is known from the subarray number.
              unsafe {
                if let (Some(start), Some(end)) = (drop_start, drop_end) {
                  subarray.drop_in_place(start, end);
                }
                subarray.destroy(subarray_capacity);
              }
            }
          }
        }

        // All individual elements have been moved out of iterator or dropped,
        // so we can defer to the `SteadyVec`'s regular destructor after
        // setting its length to zero.
        self.steady_vec.len = 0;
        // safety: we do not use the `ManuallyDrop` again after this point
        let _ = unsafe { ManuallyDrop::take(&mut self.steady_vec) };
      }
    }
  };
}
impl_steady_vec_into_iter!(SteadyVecIntoIter);
impl_steady_vec_into_iter!(BoxedSteadyVecIntoIter);
