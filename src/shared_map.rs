use std::{hash::{Hash, Hasher}, marker::PhantomData, ops::Deref, sync::{atomic::{AtomicU64, Ordering}, Arc}};

use siphasher::sip::SipHasher13;

use crate::{Ref, SharedList};

/// An experimental data structure that allows for a map to be shared
/// between threads safely.
pub struct SharedMap<K: Hash + PartialEq, V>(Arc<SharedMapInner<K, V>>);

impl <K: Hash + PartialEq + 'static, V: 'static> SharedMap<K, V> {
    /// Creates a new shared map.
    pub fn new() -> Self {
        Self(Arc::new(SharedMapInner::new()))
    }

    /// Creates a new shared map with the given number of buckets.
    pub fn with_capacity(capacity: usize) -> Self {
        Self(Arc::new(SharedMapInner::with_capacity(capacity)))
    }
}

impl <K: Hash + PartialEq, V> Deref for SharedMap<K, V> {
    type Target = SharedMapInner<K, V>;
    fn deref(&self) -> &Self::Target { &self.0 }
}

impl <K: Hash + PartialEq, V> Clone for SharedMap<K, V> {
    fn clone(&self) -> Self { Self(Arc::clone(&self.0)) }
}

impl <K: Hash + PartialEq + 'static, V: 'static> Default for SharedMap<K, V> {
    fn default() -> Self { Self::new() }
}

/// An experimental data structure that allows for a map to be shared
/// between threads safely.
pub struct SharedMapInner<K: Hash + PartialEq, V> {
    pub(crate) buckets: Box<[SharedList<(u64, K, V)>]>,
    pub(crate) capacity: AtomicU64,
    _phantom: PhantomData<K>
}

impl <K: Hash + PartialEq + 'static, V: 'static> SharedMapInner<K, V> {
    /// Creates a new shared map.
    pub fn new() -> Self {
        Self::with_capacity(16)
    }

    /// Creates a new shared map with the given number of buckets.
    pub fn with_capacity(capacity: usize) -> Self {
        // create vector of shared lists to make up our bucket array
        // the length of this vector must match the given capacity.
        let mut vec = Vec::with_capacity(capacity);
        for _ in 0 .. capacity {
            let list = SharedList::new_ordered(|old: &(u64, K, V), new: &(u64, K, V)| new.0.cmp(&old.0));
            list.set_enforce_noneq(true);
            vec.push(list);
        }

        Self {
            buckets: vec.into_boxed_slice(),
            capacity: AtomicU64::new(capacity as u64),
            _phantom: PhantomData::default()
        }
    }

    /// Returns the number of buckets in this map.
    #[allow(unused)]
    pub(crate) fn get_bucket_count(&self) -> usize {
        self.capacity.load(Ordering::Acquire) as usize
    }

    /// Get a specific bucket from this map.
    #[allow(unused)]
    pub(crate) fn get_bucket(&self, idx: usize) -> Option<SharedList<(u64, K, V)>> {
        if idx >= self.get_bucket_count() { return None }
        Some(self.buckets[idx].clone())
    }

    /// Inserts a key value pair into this map.
    pub fn insert(&self, key: K, value: V) -> bool {
        let hash = self.hash_key(&key);
        let bucket = &self.buckets[(hash % self.capacity.load(Ordering::Acquire)) as usize];
        let result = !bucket.remove_all(|a| a.0 == hash).is_empty();
        bucket.push((hash, key, value));
        result
    }

    /// Attempts to get a value for the given key stored in the map.
    /// If none is found, none is returned.
    pub fn get<'a>(&'a self, key: &K) -> Option<Ref<V>> {
        let hash = self.hash_key(&key);
        let bucket = &self.buckets[(hash % self.capacity.load(Ordering::Acquire)) as usize];
        let found = bucket.find(|a| &a.1 == key);
        if let Some(found) = found {
            Some(Ref::new(found, |entry| &entry.downcast_ref::<Ref<(u64, K, V)>>().unwrap().2))
        } else { None }
    }

    /// Attempts to find a value for the given key in the map.
    /// If none is found, false is returned.
    pub fn contains(&self, key: &K) -> bool {
        let hash = self.hash_key(&key);
        (&self.buckets[(hash % self.capacity.load(Ordering::Acquire)) as usize])
            .iter()
            .any(|content| content.0 == hash)
    }

    /// Attempts to remove a value for the given key in the map.
    /// If none is found, nothing is removed and none is returned.
    pub fn remove(&self, key: &K) -> Option<Ref<V>> {
        let hash = self.hash_key(&key);
        (&self.buckets[(hash % self.capacity.load(Ordering::Acquire)) as usize])
            .remove_search(|content| content.0 == hash)
            .map(|content| Ref::new(content, |node| &node.downcast_ref::<Ref<(u64, K, V)>>().unwrap().2))
    }

    pub fn compute_if_absent<F>(&self, key: K, func: F) -> Ref<V>
        where F: FnOnce() -> V
    {
        let hash = self.hash_key(&key);
        let bucket = &self.buckets[(hash % self.capacity.load(Ordering::Acquire)) as usize];
        
        // return item in map if found
        let found = bucket.find(|a| &a.1 == &key);
        if let Some(found) = found { return Ref::new(found, |node| &node.downcast_ref::<Ref<(u64, K, V)>>().unwrap().2) }

        // otherwise call func to create value then insert it
        let value = func();
        bucket.push((hash, key, value));

        let found = bucket
            .find(|a| &a.0 == &hash)
            .unwrap();
        return Ref::new(found, |node| &node.downcast_ref::<Ref<(u64, K, V)>>().unwrap().2);
    }

    pub fn iter<'a>(&'a self) -> Box<dyn Iterator<Item = Ref<(u64, K, V)>> + 'a> {
        Box::new(
            self.buckets.iter()
                .flat_map(|a| a.iter())
        )
    }

    /// Utility function that hashes the given key into a u64.
    pub(crate) fn hash_key(&self, key: &K) -> u64 {
        let mut hasher = SipHasher13::new_with_keys(0, 0);
        key.hash(&mut hasher);
        let hash = hasher.finish();
        return hash;
    }
}

#[cfg(test)]
mod tests {
    use crate::SharedMapInner;

    #[test]
    pub fn test_map_one() {
        let map = SharedMapInner::<i32, i32>::new();
        map.insert(32, 74563);
        map.insert(45, 984023);

        assert!(map.contains(&45));
        assert!(!map.contains(&54));
        assert!(*map.get(&45).unwrap() == 984023);
        assert!(*map.remove(&32).unwrap() == 74563);
    }
}
