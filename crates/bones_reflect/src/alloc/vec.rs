use std::borrow::Cow;

use crate::{Schema, SchemaBox, SchemaMismatchError, SchemaPtr, SchemaPtrMut};

use super::ResizableAlloc;

/// An untyped [`Vec`]-like collection.
///
/// # Important Type Data Note
///
/// All inserts into the vector are check to be [`Schema::equivalent()`], which significantly
/// _doesn't_ verify the equivalence of the schemas' [`TypeDatas`][crate::TypeDatas].
///
/// This means that if you insert an item with a different schema, that is equivalent to the
/// [`SchemaVec`]'s schema, the insert will succeed, but the type data of the inserted item will
/// be lost, and reading the item out of the [`SchemaVec`] will assume the schema and type data
/// of the [`SchemaVec`].
pub struct SchemaVec {
    /// The allocation for stored items.
    buffer: ResizableAlloc,
    /// The number of items actually stored in the vec.
    len: usize,
    /// The schema of the items stored in the vec.
    schema: Cow<'static, Schema>,
}

impl SchemaVec {
    /// Initialize an empty [`SchemaVec`] for items with the given schema.
    pub fn new<S: Into<Cow<'static, Schema>>>(schema: S) -> Self {
        let schema = schema.into();
        let layout = schema.layout_info().layout;
        Self {
            buffer: ResizableAlloc::new(layout),
            len: 0,
            schema,
        }
    }

    /// Grow the backing buffer to fit more elements.
    fn grow(&mut self) {
        let cap = self.buffer.capacity();
        if cap == 0 {
            self.buffer.resize(1).unwrap();
        } else {
            self.buffer.resize(cap * 2).unwrap();
        }
    }

    /// Push the item into the end of the vector.
    pub fn try_push(&mut self, item: SchemaBox) -> Result<(), SchemaMismatchError> {
        // Ensure matching schema
        if !self.schema.equivalent(item.schema()) {
            return Err(SchemaMismatchError);
        }

        // Make room for more elements if necessary
        if self.len == self.buffer.capacity() {
            self.grow();
        }

        // Copy the item into the vec
        unsafe {
            self.buffer
                .unchecked_idx_mut(self.len)
                .as_ptr()
                .copy_from_nonoverlapping(
                    item.as_ref().ptr().as_ptr(),
                    self.buffer.layout().size(),
                );
        }

        // Don't run the item's destructor, it's the responsibility of the vec
        item.forget();

        // Extend the length. This cannot overflow because we will run out of memory before we
        // exhaust `usize`.
        self.len += 1;

        Ok(())
    }

    /// Push the item into the end of the vector.
    #[track_caller]
    pub fn push(&mut self, item: SchemaBox) {
        self.try_push(item).unwrap()
    }

    /// Pop the last item off of the end of the vector.
    pub fn pop(&mut self) -> Option<SchemaBox> {
        if self.len == 0 {
            None
        } else {
            // Decrement our length
            self.len -= 1;

            unsafe {
                // Allocate memory for the box
                let mut b = SchemaBox::uninitialized(self.schema.clone());
                // Copy the last item in our vec to the box
                b.as_mut().ptr().as_ptr().copy_from_nonoverlapping(
                    self.buffer.unchecked_idx_mut(self.len).as_ptr(),
                    self.buffer.layout().size(),
                );

                Some(b)
            }
        }
    }

    /// Get the item with the given index.
    pub fn get(&self, idx: usize) -> Option<SchemaPtr<'_, '_>> {
        if idx >= self.len {
            None
        } else {
            let ptr = unsafe { self.buffer.unchecked_idx(idx) };

            unsafe {
                Some(SchemaPtr::from_ptr_schema(
                    ptr.as_ptr(),
                    Cow::Borrowed(self.schema.as_ref()),
                ))
            }
        }
    }

    /// Get an item with the given index.
    pub fn get_mut(&mut self, idx: usize) -> Option<SchemaPtrMut<'_, '_, '_>> {
        if idx >= self.len {
            None
        } else {
            let ptr = unsafe { self.buffer.unchecked_idx(idx) };

            unsafe {
                Some(SchemaPtrMut::from_ptr_schema(
                    ptr.as_ptr(),
                    Cow::Borrowed(self.schema.as_ref()),
                ))
            }
        }
    }

    /// Get the number of items in the vector.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if the vector has zero items in it.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get the capacity of the backing buffer.
    pub fn capacity(&self) -> usize {
        self.buffer.capacity()
    }

    /// Get the schema of items in this [`SchemaVec`].
    pub fn schema(&self) -> &Schema {
        &self.schema
    }
}

impl Drop for SchemaVec {
    fn drop(&mut self) {
        for _ in 0..self.len {
            drop(self.pop().unwrap());
        }
    }
}
