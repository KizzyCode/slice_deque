//! This library provides `SliceQueue`, a optimized queue for efficient working with (byte-)slices.
//! It allows you to
//!  - efficiently push an arbitrary amount of elements to the back by either consuming them or by
//!    cloning/copying them from a slice (if the type supports the `Clone`/`Copy` trait)
//!  - communicate and enforce a limit on the amount of elements to store
//!  - efficiently pop an arbitrary amount of elements from the front (optionally into a slice to
//!    avoid uneccessary reallocations)
//!  - access the underlying buffer directly by using (range-)indices
//!  - dereference the `SliceQueue<T>` by propagating the `deref()`-call to the underlying `Vec<T>`

use std::{
	usize, fmt::{ Debug, Formatter, Result as FmtResult },
	ops::{
		Index, IndexMut,
		Range, RangeFrom, RangeTo, RangeInclusive, RangeToInclusive, RangeBounds, Bound
	}
};
#[cfg(feature = "unsafe_fast_code")]
use std::{ ptr, mem };
#[cfg(feature = "deref")]
use std::ops::{ Deref, DerefMut };


#[derive(Default)]
pub struct SliceQueue<T> {
	backing: Vec<T>,
	limit: usize
}
impl<T> SliceQueue<T> {
	/// Creates a new `SliceQueue`
	///
	/// Returns _the new `SliceQueue`_
	pub fn new() -> Self {
		SliceQueue{ backing: Vec::new(), limit: usize::MAX }
	}
	/// Creates a new `SliceQueue` with a preallocated capacity `n`
	///
	/// Parameters:
	///  - `n`: The capacity to preallocate
	///
	/// Returns _the new `SliceQueue`_
	pub fn with_capacity(n: usize) -> Self {
		SliceQueue{ backing: Vec::with_capacity(n), limit: usize::MAX }
	}
	/// Creates a new `SliceQueue` with a predefined `limit` (the default limit is `usize::MAX`)
	///
	/// Parameters:
	///  - `limit`: The limit to enforce. The limit indicates the maximum amount of elements that
	///    can be stored by `self`.
	///
	/// Returns _the new `SliceQueue`_
	pub fn with_limit(limit: usize) -> Self {
		SliceQueue{ backing: Vec::new(), limit }
	}
	
	
	/// The amount of elements stored
	///
	/// Returns _the amount of elements stored in `self`_
	pub fn len(&self) -> usize {
		self.backing.len()
	}
	/// Checks if there are __no__ elements stored
	///
	/// Returns either _`true`_ if `self` is empty or _`false`_ otherwise
	pub fn is_empty(&self) -> bool {
		self.backing.is_empty()
	}
	
	/// Returns the allocated capacity
	///
	/// Returns _the allocated capacity of `self`_
	pub fn capacity(&self) -> usize {
		self.backing.capacity()
	}
	/// Reserves an additional amount of memory to push `additional_element_count` elements without
	/// reallocating
	///
	/// Parameters:
	///  - `additional_element_count`: The amount of elements that we should be able to append
	///    without reallocating
	pub fn reserve(&mut self, additional_element_count: usize) {
		self.backing.reserve(additional_element_count)
	}
	/// Shrinks the allocated capacity if less than it's half is used or the allocated capacity is
	/// greater than `self.limit`.
	pub fn shrink_opportunistic(&mut self) {
		// Compute the half capacity
		let half_capacity = if self.capacity() == 0 { 0 }
			else { self.capacity() / 2 };
		
		// Resize the backing if the used space is smaller than the half capacity
		if self.len() > 4 && (self.len() <= half_capacity || self.capacity() > self.limit) { self.backing.shrink_to_fit() }
	}
	/// Shrinks the allocated capacity as much as possible
	pub fn shrink_to_fit(&mut self) {
		self.backing.shrink_to_fit()
	}
	
	/// Returns the current limit
	///
	/// Returns _the current size-limit of `self`_
	pub fn limit(&self) -> usize {
		self.limit
	}
	/// Sets a new limit (the default limit is `usize::MAX`)
	///
	/// _Info: The limit is only enforced during the `push*`-calls. If the current length exceeds
	/// the new limit, nothing happens until a `push*`-call would exceed the limit._
	///
	/// Parameters:
	///  - `limit`: The new limit to enforce. The limit indicates the maximum amount of elements
	///    that can be stored by `self`.
	pub fn set_limit(&mut self, limit: usize) {
		self.limit = limit
	}
	/// Returns the amount of space remaining until `self.limit` is reached
	///
	/// Returns _the amount of space remaining in `self` until `self.limit` is reached_
	pub fn remaining(&self) -> usize {
		self.limit.checked_sub(self.len()).unwrap_or_default()
	}
	
	/// Consumes the first element and returns it
	///
	/// Returns either _`Some(element)`_ if there was an element to consume or _`None`_ otherwise
	pub fn pop(&mut self) -> Option<T> {
		match self.is_empty() {
			true => None,
			false => {
				let element = self.backing.remove(0);
				self.shrink_opportunistic();
				Some(element)
			}
		}
	}
	/// Consumes the first `n` elements and returns them
	///
	/// Parameters:
	///  - `n`: The amount of elements to consume
	///
	/// Returns either _`Some(elements)`_ if there were enough elements to consume or _`None`_
	/// otherwise
	pub fn pop_n(&mut self, n: usize) -> Option<Vec<T>> {
		if self.len() < n { return None }
		
		// Copy elements into a new vector
		#[cfg(feature = "unsafe_fast_code")]
		let elements = unsafe {
			// Create target vector
			let mut elements = Vec::with_capacity(n);
			let remaining = self.len() - n;
			
			// Copy stored elements to the new vector and the remaining elements to the front
			ptr::copy_nonoverlapping(self.backing.as_ptr(), elements.as_mut_ptr(), n);
			ptr::copy(self.backing[n..].as_ptr(), self.backing.as_mut_ptr(), remaining);
			
			// Adjust the lengths
			elements.set_len(n);
			self.backing.set_len(remaining);
			
			elements
		};
		#[cfg(not(feature = "unsafe_fast_code"))]
		let elements = /* safe */ {
			// Drain `n` elements and collect them
			self.backing.drain(..n).collect()
		};
		
		self.shrink_opportunistic();
		Some(elements)
	}
	/// Consumes the first `dst.len()` and moves them into `dst`
	///
	/// __Warning: This function panics if there are not enough elements stored to fill `dst`
	/// completely__
	///
	/// Parameters:
	///  - `dst`: The target to move the elements into
	pub fn pop_into(&mut self, dst: &mut[T]) {
		assert!(self.len() >= dst.len(), "`dst` is larger than `self`");
		
		// Copy raw data
		let to_move = dst.len();
		#[cfg(feature = "unsafe_fast_code")]
		unsafe {
			// Replace the elements in dst
			Self::replace_n(self.backing.as_ptr(), dst.as_mut_ptr(), to_move);
			
			// Move the remaining stored elements to the front and adjust length
			let remaining = self.len() - to_move;
			ptr::copy(self.backing[to_move..].as_ptr(), self.backing.as_mut_ptr(), remaining);
			self.backing.set_len(remaining);
		}
		#[cfg(not(feature = "unsafe_fast_code"))]
		/* safe */ {
			// Move `to_move` elements into `dst`
			let (mut src, dst) = (self.backing.drain(..to_move), dst.iter_mut());
			dst.for_each(|t| *t = src.next().unwrap());
		}
		
		self.shrink_opportunistic();
	}
	
	
	/// Discards the first `n` elements
	///
	/// __Warning: This function panics if there are less than `n` elements stored in `self`__
	///
	/// Parameters:
	///  - `n`: The amount of elements to discard
	pub fn discard_n(&mut self, n: usize) {
		assert!(self.len() >= n, "`n` is larger than the amount of elements in `self`");
		
		// Drop `n` elements and copy the remaining elements to the front
		#[cfg(feature = "unsafe_fast_code")]
		unsafe {
			// Move the remaining stored elements to the front and adjust the length
			let remaining = self.len() - n;
			Self::replace_n(self.backing[n..].as_ptr(), self.backing.as_mut_ptr(), remaining);
			self.backing.set_len(remaining);
		}
		#[cfg(not(feature = "unsafe_fast_code"))]
		/* safe */ {
			// Drain `n` elements from the front
			self.backing.drain(..n);
		}
		self.shrink_opportunistic();
	}
	
	
	/// Appends `element` at the end
	///
	/// __Warning: This function panics if `self.limit` is exceeded__
	///
	/// Parameters:
	///  - `element`: The element to append at the end
	pub fn push(&mut self, element: T) {
		assert!(self.limit >= self.len() + 1, "`self.len() + 1` is larger than `self.limit`");
		
		self.backing.push(element)
	}
	/// Appends `n` at the end
	///
	/// __Warning: This function panics if `self.limit` is exceeded__
	///
	/// Parameters:
	///  - `n`: The n elements to append at the end
	pub fn push_n(&mut self, mut n: Vec<T>) {
		assert!(self.limit >= self.len() + n.len(), "`self.len() + n.len()` is larger than `self.limit`");
		
		self.backing.append(&mut n);
	}
	/// Clones and appends all elements in `src` at the end
	///
	/// __Warning: This function panics if `self.limit` is exceeded__
	///
	/// Parameters:
	///  - `src`: A slice containing the elements to clone and append
	pub fn push_from(&mut self, src: &[T]) where T: Clone {
		assert!(self.limit >= self.len() + src.len(), "`self.len() + src.len()` is larger than `self.limit`");
		
		self.backing.extend_from_slice(src)
	}
	/// Calls `push_fn` to push up to `n` elements in place
	///
	/// The function works like this:
	///  1. `n` default elements are inserted at the end
	///  2. `push_fn` is called with a mutable slice referencing the new elements and returns either
	///     the amount of elements pushed or an error
	///  3. If the amount of elements pushed is smaller than `n` or an error occurred, the unused
	///     default elements are removed again
	///
	/// Example:
	/// ```
	/// # extern crate slice_queue;
	/// # use slice_queue::SliceQueue;
	///	let mut slice_queue = SliceQueue::new();
	///
	/// // Successful push
	///	slice_queue.push_in_place(7, |buffer: &mut[usize]| -> Result<usize, ()> {
	/// 	(0..4).for_each(|i| buffer[i] = i);
	/// 	Ok(4)
	/// });
	/// assert_eq!(slice_queue.len(), 4);
	/// (0..4).for_each(|i| assert_eq!(slice_queue[i], i));
	///
	/// // Failed push
	/// slice_queue.push_in_place(7, |buffer: &mut[usize]| -> Result<usize, ()> {
	/// 	(0..4).for_each(|i| buffer[i] = i + 7);
	/// 	Err(())
	/// });
	/// assert_eq!(slice_queue.len(), 4);
	/// (0..4).for_each(|i| assert_eq!(slice_queue[i], i));
	///	```
	pub fn push_in_place<E>(&mut self, n: usize, mut push_fn: impl FnMut(&mut[T]) -> Result<usize, E>) -> Result<(), E> where T: Default {
		assert!(self.limit >= self.len() + n, "`self.len() + n` is larger than `self.limit`");
		
		// Append `n` default elements
		let old_len = self.len();
		#[cfg(feature = "unsafe_fast_code")]
		unsafe {
			// Reserve `n` elements and adjust length
			self.backing.reserve(n);
			self.backing.set_len(old_len + n);
			
			// Initialize the elements with their default value
			let mut ptr = self.backing[old_len..].as_mut_ptr();
			(0..n).for_each(|_| {
				*ptr = T::default();
				ptr = ptr.offset(1);
			});
		}
		#[cfg(not(feature = "unsafe_fast_code"))]
		/* safe */ {
			(0..n).for_each(|_| self.backing.push(T::default()));
		}
		
		// Call `push_fn` and truncate the length to the amount of elements pushed
		let pushed = push_fn(&mut self.backing[old_len..]);
		self.backing.truncate(old_len + match pushed.as_ref() {
			Ok(pushed) => *pushed,
			Err(_) => 0
		});
		self.shrink_opportunistic();
		
		pushed.map(|_| ())
	}
	
	/// A private helper function to translate `RangeBounds` into ranges relative to `self`
	///
	/// __Warning: This function panics if an exclusive range over- or underflows `usize` limits__
	///
	/// Parameters:
	///  - `bounds`: The `RangeBounds` to translate
	///
	/// Returns _the translated range_
	fn range_from_bounds(&self, bounds: impl RangeBounds<usize>) -> Range<usize> {
		let start_included = match bounds.start_bound() {
			Bound::Included(b) => *b,
			Bound::Excluded(_) => unreachable!(),
			Bound::Unbounded => 0
		};
		let end_excluded = match bounds.end_bound() {
			Bound::Included(b) => if *b > usize::MIN { *b - 1 }
					else { panic!("Index usize::MIN - 1 is invalid") },
			Bound::Excluded(b) => *b,
			Bound::Unbounded => self.backing.len()
		};
		start_included..end_excluded
	}
	/// A private helper that copies `n` elements from `src` to `dst`. The elements in `dst` are
	/// dropped if necessary.
	///
	/// __Warning: Because this function operates on raw memory, YOU must take care of stuff like
	/// memory-safety, ownership and ref-counts of the copied elements etc.__
	///
	/// Parameters:
	///  - `src`: A pointer to the source elements
	///  - `dst`: A pointer to the destination
	///  - `n`: The amount of elements to copy
	#[cfg(feature = "unsafe_fast_code")]
	unsafe fn replace_n(src: *const T, dst: *mut T, n: usize) {
		// Drop elements in dst if necessary
		if mem::needs_drop::<T>() {
			let mut ptr = dst;
			(0..n).for_each(|_| {
				ptr.drop_in_place();
				ptr = ptr.offset(1);
			})
		}
		// Copy src to dst
		ptr::copy(src, dst, n);
	}
}
impl<T: Debug> Debug for SliceQueue<T> {
	fn fmt(&self, f: &mut Formatter) -> FmtResult {
		f.debug_struct("SliceQueue").field("backing", &self.backing).finish()
	}
}
impl<T> From<Vec<T>> for SliceQueue<T> {
	fn from(vec: Vec<T>) -> Self {
		SliceQueue{ backing: vec, limit: usize::MAX }
	}
}
impl<T> Clone for SliceQueue<T> where T: Clone {
	fn clone(&self) -> Self {
		SliceQueue{ backing: self.backing.clone(), limit: self.limit }
	}
}

macro_rules! impl_range_index {
    ($b:ty) => {
    	impl<T> Index<$b> for SliceQueue<T> {
    		type Output = [T];
			fn index(&self, bounds: $b) -> &[T] {
				&self.backing[self.range_from_bounds(bounds)]
			}
    	}
    	impl<T> IndexMut<$b> for SliceQueue<T> {
			fn index_mut(&mut self, bounds: $b) -> &mut [T] {
				let range = self.range_from_bounds(bounds);
				&mut self.backing[range]
			}
    	}
    };
}
impl_range_index!(Range<usize>);
impl_range_index!(RangeFrom<usize>);
impl_range_index!(RangeTo<usize>);
impl_range_index!(RangeInclusive<usize>);
impl_range_index!(RangeToInclusive<usize>);

impl<T> Index<usize> for SliceQueue<T> {
	type Output = T;
	fn index(&self, i: usize) -> &T {
		&self.backing[i]
	}
}
impl<T> IndexMut<usize> for SliceQueue<T> {
	fn index_mut(&mut self, i: usize) -> &mut T {
		&mut self.backing[i]
	}
}

#[cfg(feature = "deref")]
impl<T> Deref for SliceQueue<T> {
	type Target = <Vec<T> as Deref>::Target;
	fn deref(&self) -> &Self::Target {
		self.backing.deref()
	}
}
#[cfg(feature = "deref")]
impl<T> DerefMut for SliceQueue<T> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		self.backing.deref_mut()
	}
}


#[cfg(test)]
mod tests {
	include!("tests.rs");
}