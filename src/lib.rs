use std::{any::Any, ops::{Deref, DerefMut}, sync::{Mutex, MutexGuard}};

pub mod bit_set;
pub mod cow_data;
pub mod relaxed_mutex;
pub mod shared_list2;
pub mod shared_map;
pub mod shared_set;

pub use bit_set::*;
pub use cow_data::*;
pub use relaxed_mutex::*;
pub use shared_list2::*;
pub use shared_map::*;
pub use shared_set::*;

pub use arc_swap::*;
pub use dashmap::*;
pub use siphasher::*;

/// Useful trait for objects that must be able to be converted
/// from references to themselves to `Any` references.
pub trait AsAny {
    fn as_any(&self) -> &dyn std::any::Any;
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

pub trait SharedData<T> {
    /// Defines the type that an immutable reference of an 
    /// object can be accessed by `lock_ref`.
    type RefAccess<'a>: Deref<Target = T> where Self: 'a;

    /// Defines the type that a mutable reference of an 
    /// object can be accessed by `lock_mut`.
    type MutAccess<'a>: DerefMut<Target = T> where Self: 'a;

    /// Returns true if calling `lock_ref` may block the
    /// thread that calls `lock_ref`.  Not all implementations
    /// will block, for example, `CowData` does not.
    fn may_block_ref() -> bool;

    /// Returns true if calling `lock_mut` may block the
    /// thread that calls `lock_mut`.  Not all implementations
    /// will block, for example, `CowData` does not.
    fn may_block_mut() -> bool;
    
    /// Call to get immutable access to the contained data.
    /// This may block, call `may_block_ref` to check.
    fn lock_ref<'a>(&'a self) -> Self::RefAccess<'a>;

    /// Call to get mutable access to the contained data.
    /// This may block, call `may_block_mut` to check.
    fn lock_mut<'a>(&'a self) -> Self::MutAccess<'a>;
}

pub trait CastableSharedData<T, O>: SharedData<O> {
    /// Defines the type that an immutable reference of an 
    /// object can be accessed and then casted by `lock_cast_ref`.
    type RefCastAccess<'a>: Deref<Target = T> where Self: 'a;

    /// Defines the type that a mutable reference of an 
    /// object can be accessed and then casted by `lock_cast_mut`
    type MutCastAccess<'a>: DerefMut<Target = T> where Self: 'a;

    /// Call to get immutable casted access to the contained data.
    /// This may block, call `may_block_ref` to check.
    fn lock_cast_ref<'a>(&'a self) -> Self::RefCastAccess<'a>;
    
    /// Call to get mutable casted access to the contained data.
    /// This may block, call `may_block_mut` to check.
    fn lock_cast_mut<'a>(&'a self) -> Self::MutCastAccess<'a>;
}

impl <T> SharedData<T> for Mutex<T> {
    type RefAccess<'a> = MutexGuard<'a, T> where Self: 'a;
    type MutAccess<'a> = MutexGuard<'a, T> where Self: 'a;

    fn may_block_ref() -> bool { true }
    fn may_block_mut() -> bool { true }
    
    fn lock_ref<'a>(&'a self) -> Self::RefAccess<'a> { self.lock().expect("std mutex poisoned") }
    fn lock_mut<'a>(&'a self) -> Self::MutAccess<'a> { self.lock().expect("std mutex poisoned") }
}


/// Standard shared container for references to objects.
/// Useful for objects that track when a reference is dropped
/// for `CowData` and `RelaxedMutex`.
pub struct Ref<T> {
    data: Box<dyn Any>,
    deref: fn(&Box<dyn Any>) -> &T
}

unsafe impl <T> Send for Ref<T> {}
unsafe impl <T> Sync for Ref<T> {}

impl <T: 'static> Ref<T> {
    pub fn from_any(
        data: Box<dyn Any>,
        deref: fn(&Box<dyn Any>) -> &T
    ) -> Self {
        Self { data, deref }
    }

    pub fn from(data: T) -> Self {
        let data: Box<dyn Any> = Box::new(data);
        Self { 
            data, 
            deref: move |any| any.downcast_ref::<T>().unwrap()
        }
    }

    pub fn new<A: Any>(
        data: A,
        deref: fn(&Box<dyn Any>) -> &T
    ) -> Self {
        let data: Box<dyn Any> = Box::new(data);
        Self { 
            data, 
            deref
        }
    }
}

impl <T> Deref for Ref<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        (self.deref)(&self.data)
    }
}


/// Standard shared container for mutable references to objects.
/// Useful for objects that track when a reference is dropped
/// for `CowData` and `RelaxedMutex`.
pub struct Mut<T> {
    data: Box<dyn Any>,
    deref: Box<dyn Fn(&Box<dyn Any>) -> &T>,
    deref_mut: Box<dyn Fn(&mut Box<dyn Any>) -> &mut T>
}

unsafe impl <T> Send for Mut<T> {}
unsafe impl <T> Sync for Mut<T> {}

impl <T: 'static> Mut<T> {
    pub fn from_any(
        data: Box<dyn Any>,
        deref: fn(&Box<dyn Any>) -> &T,
        deref_mut: fn(&mut Box<dyn Any>) -> &mut T
    ) -> Self {
        Self { data, deref: Box::new(deref), deref_mut: Box::new(deref_mut) }
    }

    pub fn new<A: Any>(
        data: A,
        deref: fn(&A) -> &T,
        deref_mut: fn(&mut A) -> &mut T
    ) -> Self {
        let data: Box<dyn Any> = Box::new(data);
        Self { 
            data, 
            deref: Box::new(move |any| deref(any.downcast_ref::<A>().unwrap())) ,
            deref_mut: Box::new(move |any| deref_mut(any.downcast_mut::<A>().unwrap())) 
        }
    }
}

impl <T> Deref for Mut<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        (self.deref)(&self.data)
    }
}

impl <T> DerefMut for Mut<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        (self.deref_mut)(&mut self.data)
    }
}
