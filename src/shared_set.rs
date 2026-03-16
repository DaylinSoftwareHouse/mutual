use std::{hash::Hash, sync::Arc};

use crate::{SharedList, shared_map::SharedMapInner};

/// A set wrapper around `SharedMap`.  Allows for set like (only key) 
/// operations of `SharedMap`.
pub struct SharedSet<T: Hash + PartialEq>(Arc<SharedMapInner<T, ()>>);

impl <T: Hash + PartialEq + 'static> SharedSet<T> {
    /// Creates a new shared map.
    pub fn new() -> Self {
        Self(Arc::new(SharedMapInner::new()))
    }

    /// Creates a new shared map with the given number of buckets.
    pub fn with_capacity(capacity: usize) -> Self {
        Self(Arc::new(SharedMapInner::with_capacity(capacity)))
    }

    /// Returns the number of buckets in this map.
    #[allow(unused)]
    pub(crate) fn get_bucket_count(&self) -> usize {
        self.0.get_bucket_count()
    }

    /// Get a specific bucket from this map.
    #[allow(unused)]
    pub(crate) fn get_bucket(&self, idx: usize) -> Option<SharedList<(u64, T, ())>> {
        self.0.get_bucket(idx)
    }

    /// Inserts the given data into this set.
    pub fn insert(&self, data: T) {
        self.0.insert(data, ());
    }

    /// Returns true if this set contains the given data.
    pub fn contains(&self, data: &T) -> bool {
        self.0.contains(data)
    }

    /// Removes the given data from the set.
    /// Returns true if something was found and removed.
    /// Returns false if the given data was not in this set.
    pub fn remove(&self, data: &T) -> bool {
        self.0.remove(data).is_some()
    }
}

impl <T: Hash + PartialEq> Clone for SharedSet<T> {
    fn clone(&self) -> Self { Self(Arc::clone(&self.0)) }
}
