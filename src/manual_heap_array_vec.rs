use ::core::{
  mem::{self, MaybeUninit},
  ptr::{self, NonNull},
  slice,
};

/// A Vec-like with a fixed capacity, that is stored on the heap. The size &
/// len must be externally managed.
pub(crate) struct ManualHeapArrayVec<E> {
  data: NonNull<MaybeUninit<E>>,
}

impl<E> ManualHeapArrayVec<E> {
  pub(crate) const OPTION_NONE: Option<Self> = None;

  pub(crate) fn new(capacity: usize) -> Self {
    // todo: replace with Box<[T]>::new_uninit_slice when stable

    let mut data: Vec<MaybeUninit<E>> = Vec::with_capacity(capacity);
    // safety:
    // - new len is not greater than capacity
    // - the elements are MaybeUninit, so they need not be initialised
    unsafe { data.set_len(capacity) };

    let data = {
      let boxed_slice: Box<[MaybeUninit<E>]> = data.into_boxed_slice();
      let leaked: &mut [MaybeUninit<E>] = Box::leak(boxed_slice);
      let ptr: *mut MaybeUninit<E> = leaked.as_mut_ptr();
      // safety: `as_mut_ptr` is marked with `#[rustc_never_returns_null_ptr]`
      unsafe { NonNull::new_unchecked(ptr) }
    };

    ManualHeapArrayVec { data }
  }

  /// Set an element to the value returned from a function
  ///
  /// Drop is not called for the existing value.
  ///
  /// # Safety
  ///
  /// - `element_index` must be less than the capacity
  #[inline]
  pub(crate) unsafe fn set_with(
    &mut self,
    element_index: usize,
    f: impl FnOnce() -> E,
  ) {
    let element: &mut MaybeUninit<E> =
      unsafe { self.data.add(element_index).as_mut() };
    element.write(f());
  }

  /// Take the element from the provided index
  ///
  /// # Safety
  ///
  /// - `element_index` must be less than the capacity.
  /// - the element at `element_index` must be initialised.
  /// - after this call the element at `element_index` will be uninitialised.
  #[inline]
  pub(crate) unsafe fn take_element(&mut self, element_index: usize) -> E {
    unsafe {
      let element: &mut MaybeUninit<E> = self.data.add(element_index).as_mut();
      element.assume_init_read()
    }
  }

  /// Get the subslice from `start..=end`
  ///
  /// if `end - start == -1`, then the slice is empty.
  ///
  /// # Safety
  ///
  /// - `end` must be less than capacity.
  /// - `end - start >= -1`
  /// - elements from `start..=end` must be initialised.
  #[inline]
  pub(crate) unsafe fn as_slice(&self, start: usize, end: usize) -> &[E] {
    unsafe {
      let slice = self.as_uninit_slice(start, end);
      mem::transmute::<&[MaybeUninit<E>], &[E]>(slice)
    }
  }

  /// Get the subslice from `start..=end` mutably
  ///
  /// if `end - start == -1`, then the slice is empty.
  ///
  /// # Safety
  ///
  /// - `end` must be less than capacity.
  /// - `end - start >= -1`
  /// - elements from `start..=end` must be initialised.
  #[inline]
  pub(crate) unsafe fn as_slice_mut(
    &mut self,
    start: usize,
    end: usize,
  ) -> &mut [E] {
    unsafe {
      let slice = self.as_uninit_slice_mut(start, end);
      mem::transmute::<&mut [MaybeUninit<E>], &mut [E]>(slice)
    }
  }

  /// Get as a `&[MaybeUninit]` from `start..=end`
  ///
  /// # Safety
  ///
  /// - `end` must be less than capacity.
  /// - `end - start >= -1`
  #[inline]
  pub(crate) unsafe fn as_uninit_slice(
    &self,
    start: usize,
    end: usize,
  ) -> &[MaybeUninit<E>] {
    let len = (end as isize - start as isize + 1) as usize;
    unsafe { slice::from_raw_parts(self.data.add(start).as_ptr(), len) }
  }

  /// Get as a `&mut [MaybeUninit]` from `start..=end`
  ///
  /// note: Some elements may actually be initialised memory, and calling
  /// `MaybeUninit::write` on them will not call their Drop implementation.
  ///
  /// # Safety
  ///
  /// - `end` must be less than capacity.
  /// - `end - start >= -1`
  #[inline]
  pub(crate) unsafe fn as_uninit_slice_mut(
    &mut self,
    start: usize,
    end: usize,
  ) -> &mut [MaybeUninit<E>] {
    let len = (end as isize - start as isize + 1) as usize;
    unsafe { slice::from_raw_parts_mut(self.data.add(start).as_ptr(), len) }
  }

  /// Drop in place all elements in the subslice from `start..=end`
  ///
  /// # Safety
  ///
  /// - `end` must be less than capacity.
  /// - `end - start >= -1`
  /// - elements from `start..=end` must be initialised.
  /// - after calling this, elements from `start..=end` will be uninitialised.
  #[inline]
  pub(crate) unsafe fn drop_in_place(&mut self, start: usize, end: usize) {
    unsafe {
      let slice = self.as_slice_mut(start, end);
      ptr::drop_in_place(slice);
    }
  }

  /// Free the allocation backing this ManualHeapArrayVec
  ///
  /// Note that the Drop implementation will not be called for any elements, so
  /// they may leak. Use [`Self::drop_in_place`] first, in order to call the
  /// destructor for elements known to be initialised.
  ///
  /// # Safety
  ///
  /// - `capacity` must be equal to the capacity specified when initially
  ///   created (through `new`).
  #[inline]
  pub(crate) unsafe fn destroy(self, capacity: usize) {
    unsafe {
      let slice: &mut [MaybeUninit<E>] =
        slice::from_raw_parts_mut(self.data.as_ptr(), capacity);
      let _: Box<[MaybeUninit<E>]> = Box::from_raw(slice);
    };
  }
}
