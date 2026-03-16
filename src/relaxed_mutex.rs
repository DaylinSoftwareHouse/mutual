use std::{cell::UnsafeCell, fmt::Debug, marker::PhantomData, ops::{Deref, DerefMut}, sync::{Arc, atomic::{AtomicBool, AtomicUsize, Ordering}}};

use crate::{AsAny, CastableSharedData, SharedData};

/// A special mutex that allows for infinite immutable access
/// to an object but only one mutable access at a time.  If a
/// mutable access is requested, the mutex will wait until all
/// immutable access has been dropped and block any further
/// immutable access' until the mutable guard is dropped.
pub struct RelaxedMutex<T> {
    inner: Arc<RelaxedMutexInner<T>>
}

impl <T> RelaxedMutex<T> {
    pub fn new(data: T) -> Self {
        Self {
            inner: Arc::new(RelaxedMutexInner::new(data))
        }
    }
}

impl <T> Clone for RelaxedMutex<T> {
    fn clone(&self) -> Self {
        Self { inner: Arc::clone(&self.inner) }
    }
}

impl <T: Default> Default for RelaxedMutex<T> {
    fn default() -> Self {
        Self { inner: Arc::new(RelaxedMutexInner::new(T::default())) }
    }
}

impl <T> Deref for RelaxedMutex<T> {
    type Target = RelaxedMutexInner<T>;
    fn deref(&self) -> &Self::Target {
        &*self.inner
    }
}

impl <T: Debug> Debug for RelaxedMutex<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        RelaxedMutexInner::fmt(&self, f)
    }
}

/// A special mutex that allows for infinite immutable access
/// to an object but only one mutable access at a time.  If a
/// mutable access is requested, the mutex will wait until all
/// immutable access has been dropped and block any further
/// immutable access' until the mutable guard is dropped.
pub struct RelaxedMutexInner<T> {
    data: UnsafeCell<T>,
    // ref_access: Mutex<HashSet<ThreadId>>,
    refs: AtomicUsize,
    locked: AtomicBool
}

unsafe impl<T: Send> Sync for RelaxedMutexInner<T> {}
unsafe impl<T: Send> Send for RelaxedMutexInner<T> {}

impl <T: Debug> Debug for RelaxedMutexInner<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("RelaxedMutex")
            .field(unsafe { &*self.data.get() })
            .finish()
    }
}

impl <T> RelaxedMutexInner<T> {
    /// Creates a new mutex that with the given data.
    pub fn new(data: T) -> RelaxedMutexInner<T> {
        RelaxedMutexInner { 
            data: UnsafeCell::new(data), 
            // ref_access: Mutex::new(HashSet::new()),
            refs: AtomicUsize::new(0),
            locked: AtomicBool::new(false) 
        }
    }

    /// Returns true if the current thread is locked
    pub fn is_locked(&self) -> bool {
        return self.locked.load(Ordering::Acquire);
    }

    /// Returns true if the current thread is using this mutex.
    pub fn current_thread_using(&self) -> bool {
        // return self.ref_access.lock().unwrap().contains(&std::thread::current().id());
        return false;
    }

    fn create_ref_lock(&self) {
        // spin lock if mutable locked
        loop {
            if !self.locked.load(Ordering::Acquire) {
                break;
            }

            std::thread::yield_now();
        }

        {
            self.refs.fetch_add(1, Ordering::Acquire);
        }
    }

    fn create_mut_lock(&self) {
        // spin lock until mutable lock achieved
        loop {
            if !self.locked.swap(true, Ordering::Acquire) {
                break;
            }

            std::thread::yield_now();
        }

        // add to ref access
        {
            self.refs.fetch_add(1, Ordering::Acquire);
        }

        // wait until ref access is just this function
        {
            loop {
                let count = self.refs.load(Ordering::Acquire);
                if count <= 1 {
                    break;
                }
                std::thread::yield_now();
            }
        }
    }
}

impl <T> SharedData<T> for RelaxedMutex<T> {
    type RefAccess<'a> = RefGuard<T> where Self: 'a;
    type MutAccess<'a> = MutGuard<T> where Self: 'a;

    fn may_block_ref() -> bool { true }
    fn may_block_mut() -> bool { true }

    /// Obtains an immutable guard to this mutex.
    /// This function will block if a mutable guard has
    /// not been dropped for this mutex.
    fn lock_ref(&self) -> RefGuard<T> {
        self.create_ref_lock();
        RefGuard { mutex: self.inner.clone() }
    }

    /// Obtains a mutable guard to this mutex.
    /// This function will block if a mutable guard has
    /// not been dropped for this mutex and until all
    /// pre-existing immutable guards have been dropped
    /// for this mutex.  The mutable guard will block any
    /// new immutable guard creations until it is dropped.
    fn lock_mut(&self) -> MutGuard<T> {
        self.create_mut_lock();
        MutGuard { mutex: self.inner.clone() }
    }
}

impl <O: AsAny, N: 'static> CastableSharedData<N, O> for RelaxedMutex<O> {
    type RefCastAccess<'a> = RefCastGuard<O, N> where Self: 'a;
    type MutCastAccess<'a> = MutCastGuard<O, N> where Self: 'a;

    /// Obtains an casted immutable guard to this mutex.
    /// This function will block if a mutable guard has
    /// not been dropped for this mutex.
    fn lock_cast_ref(&self) -> Self::RefCastAccess<'_> {
        self.create_ref_lock();
        RefCastGuard { mutex: self.inner.clone(), _phantom: PhantomData::default() }
    }

    /// Obtains a casted mutable guard to this mutex.
    /// This function will block if a mutable guard has
    /// not been dropped for this mutex and until all
    /// pre-existing immutable guards have been dropped
    /// for this mutex.  The mutable guard will block any
    /// new immutable guard creations until it is dropped.
    fn lock_cast_mut(&self) -> Self::MutCastAccess<'_> {
        self.create_mut_lock();
        MutCastGuard { mutex: self.inner.clone(), _phantom: PhantomData::default() }
    }
}

/// An immutable guard to the contained data in the mutex 
/// this was obtained from.  Any instances of this guard
/// will block the creation of mutable guards until they
/// are all dropped.
pub struct RefGuard<T> {
    mutex: Arc<RelaxedMutexInner<T>>
}

impl <T> Deref for RefGuard<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.mutex.data.get() }
    }
}

impl <T> Drop for RefGuard<T> {
    fn drop(&mut self) {
        // // drop from ref access tracker
        // self.mutex.ref_access
        //     .lock()
        //     .unwrap()
        //     .remove(&std::thread::current().id());
        self.mutex.refs.fetch_sub(1, Ordering::Acquire);
    }
}

/// An immutable guard to the contained data casted to the [N]ew type 
/// in the mutex this was obtained from.  Any instances of this guard
/// will block the creation of mutable guards until they are all dropped.
pub struct RefCastGuard<O: AsAny, N: 'static> {
    mutex: Arc<RelaxedMutexInner<O>>,
    _phantom: PhantomData<N>
}

impl <O: AsAny, N: 'static> Deref for RefCastGuard<O, N> {
    type Target = N;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.mutex.data.get() }
            .as_any()
            .downcast_ref::<N>()
            .unwrap()
    }
}

impl <O: AsAny, N: 'static> Drop for RefCastGuard<O, N> {
    fn drop(&mut self) {
        // drop from ref access tracker
        // self.mutex.ref_access
        //     .lock()
        //     .unwrap()
        //     .remove(&std::thread::current().id());
        self.mutex.refs.fetch_sub(1, Ordering::Acquire);
    }
}

/// An mutable guard to the contained data in the mutex 
/// this was obtained from.  Any instances of this guard
/// will block the creation of mutable or immutable guards 
/// until they are all dropped.
pub struct MutGuard<T> {
    mutex: Arc<RelaxedMutexInner<T>>
}

impl <T> Deref for MutGuard<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.mutex.data.get() }
    }
}

impl <'a, T> DerefMut for MutGuard<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.mutex.data.get() }
    }
}

impl <T> Drop for MutGuard<T> {
    fn drop(&mut self) {
        // drop mutable lock
        self.mutex.locked.swap(false, Ordering::Release);
        
        // remove from ref access tracker
        // self.mutex.ref_access
        //     .lock()
        //     .unwrap()
        //     .remove(&std::thread::current().id());
        self.mutex.refs.fetch_sub(1, Ordering::Acquire);
    }
}

/// An mutable guard to the contained data down casted to the [N]ew generic 
/// type in the mutex  this was obtained from. Any instances of this guard
/// will block the creation of mutable or immutable guards until they are 
/// all dropped.
pub struct MutCastGuard<O: AsAny, N: 'static> {
    mutex: Arc<RelaxedMutexInner<O>>,
    _phantom: PhantomData<N>
}

impl <O: AsAny, N: 'static> Deref for MutCastGuard<O, N> {
    type Target = N;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.mutex.data.get() }
            .as_any()
            .downcast_ref::<N>()
            .unwrap()
    }
}

impl <O: AsAny, N: 'static> DerefMut for MutCastGuard<O, N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.mutex.data.get() }
            .as_any_mut()
            .downcast_mut::<N>()
            .unwrap()
    }
}

impl <O: AsAny, N: 'static> Drop for MutCastGuard<O, N> {
    fn drop(&mut self) {
        // drop mutable lock
        self.mutex.locked.swap(false, Ordering::Release);
        
        // remove from ref access tracker
        self.mutex.refs.fetch_sub(1, Ordering::Acquire);
    }
}
