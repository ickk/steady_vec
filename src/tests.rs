use super::*;

#[test]
fn meta() {
  use super::{index_to_subarray_n, subarray_capacity, subarray_index_range};

  assert_eq!(subarray_capacity(0), 2);
  assert_eq!(subarray_index_range(0), (0, 1));
  for index in 0..=1 {
    assert_eq!(index_to_subarray_n(index), 0);
  }

  assert_eq!(subarray_capacity(1), 2);
  assert_eq!(subarray_index_range(1), (2, 3));
  for index in 2..=3 {
    assert_eq!(1, index_to_subarray_n(index));
  }

  assert_eq!(subarray_capacity(2), 4);
  assert_eq!(subarray_index_range(2), (4, 7));
  for index in 4..=7 {
    assert_eq!(2, index_to_subarray_n(index));
  }

  assert_eq!(subarray_capacity(3), 8);
  assert_eq!(subarray_index_range(3), (8, 15));
  for index in 8..=15 {
    assert_eq!(3, index_to_subarray_n(index));
  }

  assert_eq!(subarray_capacity(4), 16);
  assert_eq!(subarray_index_range(4), (16, 31));
  for index in 16..=31 {
    assert_eq!(4, index_to_subarray_n(index));
  }

  assert_eq!(subarray_capacity(31), 2usize.pow(31));
  assert_eq!(
    subarray_index_range(31),
    (2usize.pow(31), 2usize.pow(32) - 1)
  );
  assert_eq!(31, index_to_subarray_n(2usize.pow(31)));
  assert_eq!(31, index_to_subarray_n(2usize.pow(32) - 1));
}

#[test]
fn smoke() {
  let mut v: SteadyVec<usize> = SteadyVec::new();
  assert_eq!(v.is_empty(), true);
  assert_eq!(v.len(), 0);

  // test push & pop
  v.push(1337);
  assert_eq!(v.is_empty(), false);
  assert_eq!(v.len(), 1);

  v.push(42);
  assert_eq!(v.len(), 2);

  let e = v.pop();
  assert_eq!(e, Some(42));
  assert_eq!(v.len(), 1);

  v.clear();
  assert_eq!(v.len(), 0);
  for i in 0..1000 {
    v.push(i * i);
    assert_eq!(v.len(), i + 1)
  }

  // test capacity
  assert_eq!(v.len(), 1000);
  assert_eq!(v.capacity(), 1024);
  for i in 1000..2000 {
    v.push(i * i);
    assert_eq!(v.len(), i + 1)
  }
  assert_eq!(v.len(), 2000);
  assert_eq!(v.capacity(), 2048);

  // test iter
  for (i, e) in v.iter().enumerate() {
    let i2 = i * i;
    assert_eq!(&i2, e);
  }

  // test iter_mut
  for e in v.iter_mut() {
    *e *= 3;
  }
  for (i, e) in v.iter().enumerate() {
    let a = (i * i) * 3;
    assert_eq!(&a, e);
  }

  // test double ended iter
  for (i, e) in ::core::iter::zip((1000..2000).rev(), v.iter().rev()) {
    let a = (i * i) * 3;
    assert_eq!(&a, e);
  }

  // test get & get_mut
  let a = v.get(5);
  assert_eq!(a, Some(&(5 * 5 * 3)));
  let b = v.get_mut(347);
  assert_eq!(b, Some(&mut (347 * 347 * 3)));
  if let Some(b) = b {
    *b = 42;
  }
  let c = v.get(347);
  assert_eq!(c, Some(&42));

  // test Clone - should test with a non-trivial Clone, like Arc
  let v_clone = v.clone();
  assert_eq!(v_clone.len(), v.len());
  for i in 0..v.len() {
    assert_eq!(v_clone.get(i), v.get(i))
  }

  // test into iter
  for (i, e) in v.clone().into_iter().enumerate() {
    let a;
    if i == 347 {
      a = 42;
    } else {
      a = (i * i) * 3;
    }
    assert_eq!(a, e);
  }
  {
    let mut x = SteadyVec::new();
    x.extend(0..1000);
    for (i, e) in x.into_iter().enumerate().take(100) {
      assert_eq!(i, e);
    }
  }

  // test insert & remove
  {
    let mut w = SteadyVec::new();
    w.extend(0..1000);
    let mut r = w.clone();
    r.remove(123);
    for (a, b) in ::core::iter::zip(r.iter().take(123), w.iter().take(123)) {
      assert_eq!(a, b);
    }
    for (a, b) in ::core::iter::zip(r.iter().skip(123), w.iter().skip(124)) {
      assert_eq!(a, b);
    }
    r.insert(123, 123);
    for (a, b) in ::core::iter::zip(r.iter(), w.iter()) {
      assert_eq!(a, b);
    }
  }
}
