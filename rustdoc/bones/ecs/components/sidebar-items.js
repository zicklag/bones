window.SIDEBAR_ITEMS = {"struct":[["AtomicComponentStore","A typed, wrapper handle around [`UntypedComponentStore`] that is runtime borrow checked and can be cheaply cloned. Think can think of it like an `Arc<RwLock<ComponentStore>>`."],["AtomicComponentStoreRef","A read-only borrow of [`AtomicComponentStore`]."],["AtomicComponentStoreRefMut","A mutable borrow of [`AtomicComponentStore`]."],["ComponentBitsetIterator","Read-only iterator over components matching a given bitset"],["ComponentBitsetIteratorMut","Mutable iterator over components matching a given bitset"],["ComponentStore","A typed wrapper around [`UntypedComponentStore`]."],["ComponentStores","A collection of [`ComponentStore<T>`]."],["TypedComponentOps","Implements typed operations on top of a [`UntypedComponentStore`]."],["UntypedComponentBitsetIterator","Iterates over components using a provided bitset. Each time the bitset has a 1 in index i, the iterator will fetch data from the storage at index i and return it as an `Option`."],["UntypedComponentBitsetIteratorMut","Iterates over components using a provided bitset. Each time the bitset has a 1 in index i, the iterator will fetch data from the storage at index i and return it as an `Option`."],["UntypedComponentStore","Holds components of a given type indexed by `Entity`."]]};