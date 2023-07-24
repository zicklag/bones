use std::{
    alloc::{self, handle_alloc_error, Layout, LayoutError},
    ptr::NonNull,
};

use bones_utils::{Ptr, PtrMut};

use super::layout::*;

/// A low-level memory allocation utility for creating a resizable buffer of elements of a specific
/// layout.
///
/// The allocation has a capacity measured in the number of elements with the given [`Layout`] that
/// it has room for.
///
/// Dropping a [`ResizableAlloc`] will de-allocate it's memory.
pub struct ResizableAlloc {
    /// The pointer to the allocation. May be dangling for a capacity of zero or for a zero-sized
    /// layout.
    ptr: NonNull<u8>,
    /// The layout of the items stored
    layout: Layout,
    /// The layout of the items stored, with it's size padded to its alignment.
    padded: Layout,
    /// The current capacity measured in items.
    cap: usize,
}

impl ResizableAlloc {
    /// Create a new [`ResizableAlloc`] for the given memory layout. Does not actually allocate
    /// anything yet.hing.
    ///
    /// If the new capacity is greater, it will reallocate and extend the allocated region to be
    /// able to fit `new_capacity` items of the this [`ResizableAlloc`]'s layout.
    ///
    /// If the new capacity is lower, it will reallocate and remove all items
    ///
    /// The capacity will be 0 and the pointer will be dangling.
    #[inline]
    pub fn new(layout: Layout) -> Self {
        Self {
            ptr: Self::dangling(&layout),
            layout,
            padded: layout.pad_to_align(),
            cap: 0,
        }
    }

    /// Resize the buffer, re-allocating it's memory.
    pub fn resize(&mut self, new_capacity: usize) -> Result<(), LayoutError> {
        // Don't do anything for an equal new_capacity
        if self.cap == new_capacity {
            return Ok(());
        }

        // For ZSTs, simply update the capacity, the pointer will still be dangling.
        if self.layout.size() == 0 {
            self.cap = new_capacity;
            return Ok(());
        }

        // Record the old capacity.
        let old_capacity = self.cap;

        // Update our capacity to the new capacity.
        self.cap = new_capacity;

        // If we are clearing our allocation
        if new_capacity == 0 {
            // If we have existing memory to de-allocate
            if old_capacity > 0 {
                // Calculate the layout of our old allocation
                let old_alloc_layout = self.layout.repeat(old_capacity)?.0;

                // Deallocate the old memory
                unsafe { alloc::dealloc(self.ptr.as_ptr(), old_alloc_layout) }
            }

            // Update our pointer to be dangling.
            self.ptr = Self::dangling(&self.layout);

        // If we are allocating/reallocating
        } else {
            // If we have exsting memory to re-allocate
            if old_capacity > 0 {
                let old_alloc_layout = self.layout.repeat(old_capacity).unwrap().0;
                let new_alloc_layout = self.layout.repeat(new_capacity).unwrap().0;
                self.ptr = NonNull::new(unsafe {
                    alloc::realloc(self.ptr.as_ptr(), old_alloc_layout, new_alloc_layout.size())
                })
                .unwrap_or_else(|| handle_alloc_error(new_alloc_layout));

            // If we need to allocate new memory
            } else {
                let alloc_layout = self.layout.repeat(new_capacity).unwrap().0;
                self.ptr = NonNull::new(unsafe { alloc::alloc(alloc_layout) })
                    .unwrap_or_else(|| handle_alloc_error(alloc_layout));
            }
        }

        Ok(())
    }

    /// Get the layout.
    #[inline]
    pub fn layout(&self) -> Layout {
        self.layout
    }

    /// Get the capacity.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.cap
    }

    /// Get the pointer to the allocation
    #[inline]
    pub fn ptr(&mut self) -> PtrMut<'_> {
        unsafe { PtrMut::new(self.ptr) }
    }

    /// Get a pointer to the item with the given index without performing any bounds checks.
    ///
    /// # Safety
    ///
    /// This does no checks that the index is within bounds.
    #[inline]
    pub unsafe fn unchecked_idx_mut(&mut self, idx: usize) -> PtrMut<'_> {
        PtrMut::new(NonNull::new_unchecked(
            self.ptr.as_ptr().add(self.padded.size() * idx),
        ))
    }

    /// Get a pointer to the item with the given index without performing any bounds checks.
    ///
    /// # Safety
    ///
    /// This does no checks that the index is within bounds.
    #[inline]
    pub unsafe fn unchecked_idx(&self, idx: usize) -> Ptr<'_> {
        Ptr::new(NonNull::new_unchecked(
            self.ptr.as_ptr().add(self.padded.size() * idx),
        ))
    }

    /// Helper to create a dangling pointer that is properly aligned for our layout.
    #[inline]
    fn dangling(layout: &Layout) -> NonNull<u8> {
        // SOUND: the layout ensures a non-zero alignment.
        unsafe { NonNull::new_unchecked(sptr::invalid_mut(layout.align())) }
    }
}

impl Drop for ResizableAlloc {
    fn drop(&mut self) {
        if self.cap > 0 {
            unsafe { alloc::dealloc(self.ptr.as_ptr(), self.layout.repeat(self.cap).unwrap().0) }
        }
    }
}

#[cfg(test)]
mod test {
    use std::alloc::Layout;

    use crate::alloc::ResizableAlloc;

    #[test]
    fn resizable_allocation() {
        // Create the layout of the type we want to store.
        type Ty = (u32, u8);
        let layout = Layout::new::<Ty>();

        // This doesn't allocate yet
        let mut a = ResizableAlloc::new(layout);

        // We can now use resize() to allocate memory for 3 elements.
        a.resize(3).unwrap();

        // We write some data.
        for i in 0..3 {
            unsafe {
                a.ptr().as_ptr().cast::<Ty>().add(i).write((i as _, i as _));
            }
        }
        unsafe {
            assert_eq!((0, 0), (a.ptr().as_ptr() as *mut Ty).read());
            assert_eq!((1, 1), (a.ptr().as_ptr() as *mut Ty).add(1).read());
            assert_eq!((2, 2), (a.ptr().as_ptr() as *mut Ty).add(2).read());
        }

        // We can grow the allocation by resizing
        a.resize(4).unwrap();

        // And write to the new data
        unsafe {
            a.ptr().as_ptr().cast::<Ty>().add(3).write((3, 3));

            // The previous values will be there
            assert_eq!((0, 0), (a.ptr().as_ptr() as *mut Ty).read());
            assert_eq!((1, 1), (a.ptr().as_ptr() as *mut Ty).add(1).read());
            assert_eq!((2, 2), (a.ptr().as_ptr() as *mut Ty).add(2).read());
            // As well as the new one
            assert_eq!((3, 3), (a.ptr().as_ptr() as *mut Ty).add(3).read());
        }

        // We can shrink the allocation, too, which will delete the items at the end without dropping them, keeping the
        // items at the beginning.
        a.resize(1).unwrap();
        unsafe {
            assert_eq!((0, 0), (a.ptr().as_ptr() as *mut Ty).read());
        }

        // And we can delete all the items by resizing to zero ( again, this doesn't drop item, just
        // removes their memory ).
        a.resize(0).unwrap();

        // Now the pointer will be dangling, but aligned to our layout
        assert_eq!(a.ptr().as_ptr() as usize, layout.align());
    }
}
