use std::{fmt::Debug, ops::{Deref, DerefMut}, sync::{Arc, atomic::{AtomicBool, Ordering}}};

use arc_swap::ArcSwap;

use crate::{SharedData};

/// A special data structure that allows for infinite immutable access to data contained.
/// However, when data is used mutably the data is updated afterward in such a way that old
/// immutable access' will continue to reference the old data but new access' will reference
/// the new data.  Mutable access are optionally lock protected so when mutable access is requested,
/// it will be blocked until previous mutable locks are dropped.  When this lock is not enabled,
/// beware that mutable changes can override eachother depending on whichever completes last.
/// 
/// Note: This is a wrapper around ArcSwap with some extra features to make them easier to work with.
pub struct CowData<T> {
    inner: Arc<ArcSwap<Option<T>>>,
    mut_lock: Arc<AtomicBool>,
    imm_lock: Arc<AtomicBool>,
    is_mut_lock_protected: bool
}

impl <T> CowData<T> {
    pub fn new(data: T) -> Self {
        Self {
            inner: Arc::new(ArcSwap::new(Arc::new(Some(data)))),
            mut_lock: Arc::new(AtomicBool::new(false)),
            imm_lock: Arc::new(AtomicBool::new(false)),
            is_mut_lock_protected: false
        }
    }

    pub fn null() -> Self {
        Self {
            inner: Arc::new(ArcSwap::new(Arc::new(None))),
            mut_lock: Arc::new(AtomicBool::new(false)),
            imm_lock: Arc::new(AtomicBool::new(false)),
            is_mut_lock_protected: false
        }
    }

    pub fn from_arc(value: Arc<Option<T>>) -> Self {
        Self {
            inner: Arc::new(ArcSwap::new(value)),
            mut_lock: Arc::new(AtomicBool::new(false)),
            imm_lock: Arc::new(AtomicBool::new(false)),
            is_mut_lock_protected: false
        }
    }

    pub fn new_lock_protected(data: T) -> Self {
        Self {
            inner: Arc::new(ArcSwap::new(Arc::new(Some(data)))),
            mut_lock: Arc::new(AtomicBool::new(false)),
            imm_lock: Arc::new(AtomicBool::new(false)),
            is_mut_lock_protected: true
        }
    }

    pub fn null_lock_protected() -> Self {
        Self {
            inner: Arc::new(ArcSwap::new(Arc::new(None))),
            mut_lock: Arc::new(AtomicBool::new(false)),
            imm_lock: Arc::new(AtomicBool::new(false)),
            is_mut_lock_protected: true
        }
    }

    pub fn from_arc_lock_protected(value: Arc<Option<T>>) -> Self {
        Self {
            inner: Arc::new(ArcSwap::new(value)),
            mut_lock: Arc::new(AtomicBool::new(false)),
            imm_lock: Arc::new(AtomicBool::new(false)),
            is_mut_lock_protected: true
        }
    }

    /// Returns true if the internal data of this structure has not been set.
    pub fn is_null(&self) -> bool {
        while self.imm_lock.load(Ordering::Acquire) { std::thread::yield_now(); }
        return self.inner.load().is_none();
    }

    /// Returns true if the internal mutable lock is enabled.  This does not respect
    /// the `is_mut_lock_protected` flag.
    pub fn is_mut_locked(&self) -> bool {
        self.mut_lock.load(Ordering::Acquire)
    }

    /// Sets the internal data of this data structure.  Beware, if this
    /// structure is not mutable lock protected, this data could be overriden
    /// but something making changes off of an old immutable reference.
    pub fn set(&self, data: T) {
        // spin-lock until lock achieved
        if self.is_mut_lock_protected {
            loop {
                if !self.mut_lock.swap(true, Ordering::Acquire) { break }
                std::thread::yield_now();
            }
        }

        while !self.imm_lock.swap(true, Ordering::Release) { std::thread::yield_now(); }
        self.inner.store(Arc::new(Some(data)));
        self.imm_lock.store(false, Ordering::Release);

        // unlock
        if self.is_mut_lock_protected {
            self.mut_lock.store(false, Ordering::Release);
        }
    }

    /// Sets the internal pointers to null
    pub fn set_null(&self) {
        // spin-lock until lock achieved
        if self.is_mut_lock_protected {
            loop {
                if !self.mut_lock.swap(true, Ordering::Acquire) { break }
                std::thread::yield_now();
            }
        }

        while !self.imm_lock.swap(true, Ordering::Release) { std::thread::yield_now(); }
        self.inner.store(Arc::new(None));
        self.imm_lock.store(false, Ordering::Release);

        // unlock
        if self.is_mut_lock_protected {
            self.mut_lock.store(false, Ordering::Release);
        }
    }

    /// Gets the multiarc inside this data structure
    pub fn get_arc_swap(&self) -> &ArcSwap<Option<T>> { &self.inner }

    /// Returns true if this mut lock is enabled.
    pub fn is_locked(&self) -> bool {
        if !self.is_mut_lock_protected { return false }
        return self.mut_lock.load(Ordering::Acquire);
    }

    /// Create a mutability lock, this will only occur with mut lock enabled.
    pub fn create_lock(&self) {
        if self.is_mut_lock_protected {
            loop {
                if !self.mut_lock.swap(true, Ordering::Acquire) { break }
                std::thread::yield_now();
            }
        }
    }

    /// Removes an existing mutability lock, this will only occur with mut lock enabled.
    pub fn remove_lock(&self) {
        if self.is_mut_lock_protected {
            self.mut_lock.store(false, Ordering::Release);
        }
    }

    /// Takes the data in this object and moves it to the null cow, then replaces the data
    /// in this cow with the new data.  Useful for linked list.
    pub fn bump_into_null(&self, null_cow: &CowData<T>, new_data: T) {
        if !null_cow.is_null() { panic!("bump_into_null as null_cow was not null") }

        // spin-lock until lock achieved
        if self.is_mut_lock_protected {
            loop {
                if !self.mut_lock.swap(true, Ordering::Acquire) { break }
                std::thread::yield_now();
            }
        }

        // perform swaps
        while !self.imm_lock.swap(true, Ordering::Release) { std::thread::yield_now(); }
        let old_data = self.inner.swap(Arc::new(Some(new_data)));
        null_cow.inner.store(old_data);
        self.imm_lock.store(false, Ordering::Release);

        // unlock
        if self.is_mut_lock_protected {
            self.mut_lock.store(false, Ordering::Release);
        }
    }

    /// Swaps the data between two `CowData`s
    pub fn swap(&self, from: &CowData<T>) -> Arc<std::option::Option<T>> {
        while !self.imm_lock.swap(true, Ordering::Release) { std::thread::yield_now(); }
        while !from.imm_lock.swap(true, Ordering::Release) { std::thread::yield_now(); }
        let from_old = from.inner.load_full().clone();
        let self_old = self.inner.swap(from_old);
        from.imm_lock.store(false, Ordering::Release);
        self.imm_lock.store(false, Ordering::Release);
        from.inner.swap(self_old)
    }

    /// Returns a reference guard to the internal data.
    pub fn get_ref(&self) -> RefCowData<T> {
        if self.is_null() { 
            panic!("Cannot get_ref to null cow data") 
        }
        RefCowData { node: self.inner.load_full().clone() }
    }
}

impl <T: Clone> CowData<T> {
    /// Returns a mutable guard to the internal data.
    /// Beware, once you make the first change, this guard references
    /// its own copy of data, not what is stored in the parent `CowData`.
    /// This can lead to lost data if the parent is not mutable lock protected.
    pub fn get_mut(&self) -> MutCowData<T> {
        if self.is_null() { panic!("Cannot lock_mut to null cow data") }
        
        // spin-lock until lock achieved
        if self.is_mut_lock_protected {
            loop {
                if !self.mut_lock.swap(true, Ordering::Acquire) { break }
                std::thread::yield_now();
            }
        }

        // build and return guard
        let data = Option::clone(&*self.inner.load_full());
        return MutCowData { original_structure: Arc::clone(&self.inner), data }
    }
}

impl <T: Clone> SharedData<T> for CowData<T> {
    type RefAccess<'a> = RefCowData<T> where Self: 'a;
    type MutAccess<'a> = MutCowData<T> where Self: 'a;

    fn may_block_ref() -> bool { false }
    fn may_block_mut() -> bool { false }

    fn lock_ref<'a>(&'a self) -> Self::RefAccess<'a> { self.get_ref() }
    fn lock_mut<'a>(&'a self) -> Self::MutAccess<'a> { self.get_mut() }
}

impl <T> Default for CowData<T> {
    fn default() -> Self {
        Self::null()
    }
}

impl <T: Debug> Debug for CowData<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_null() {
            f.debug_struct("CowData")
                .field("data", &"null")
                .finish()
        } else {
            f.debug_struct("CowData")
                .field("data", &*self.get_ref())
                .finish()
        }
    }
}

impl <T> Clone for CowData<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            mut_lock: Arc::clone(&self.mut_lock),
            imm_lock: Arc::clone(&self.imm_lock),
            is_mut_lock_protected: self.is_mut_lock_protected
        }
    }
}

unsafe impl <T> Send for CowData<T> {}
unsafe impl <T> Sync for CowData<T> {}

pub struct RefCowData<T> {
    node: Arc<Option<T>>
}

impl <T> Deref for RefCowData<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target { &(*self.node).as_ref().unwrap() }
}

pub struct MutCowData<T: Clone> {
    original_structure: Arc<ArcSwap<Option<T>>>,
    data: Option<T>
}

impl <T: Clone> MutCowData<T> {
    pub fn set(&mut self, data: T) {
        self.data = Some(data);
    }
}

impl <T: Clone> Deref for MutCowData<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        self.data.as_ref().unwrap()
    }
}

impl <T: Clone> DerefMut for MutCowData<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.data.as_mut().unwrap()
    }
}

impl <T: Clone> Drop for MutCowData<T> {
    fn drop(&mut self) {
        let data = self.data.take().unwrap();
        self.original_structure.store(Arc::new(Some(data)));
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use crate::CowData;

    #[test]
    pub fn cow_thread_test_get_set() {
        let cow = CowData::new(3.0);

        for _ in 0 .. 32 {
            let cow_local = cow.clone();

            std::thread::spawn(move || {
                let start_time = Instant::now();

                while Instant::now().duration_since(start_time) < Duration::from_secs(20) {
                    let next = *cow_local.get_ref() + 1.0;
                    cow_local.set(next);
                }
            });
        }
    }

    #[test]
    pub fn shared_test_one() {
        let data = CowData::new(2_u32);
        {
            let data2 = data.clone();
            *(data2.get_mut()) = 3;
        }

        assert!(*data.get_ref() == 3_u32);
    }

    #[test]
    pub fn shared_test_two() {
        let data = CowData::new(2_u32);
        let a = *data.get_ref();
        assert!(a == 2);

        let data2 = data.clone();
        let guard = data.get_ref();
        *(data2.get_mut()) = 3;

        let guard2 = data.get_ref();
        data2.set(1);

        assert!(*guard == 2_u32);
        assert!(*guard2 == 3_u32);
        assert!(*data.get_ref() == 1_u32);
    }
}
