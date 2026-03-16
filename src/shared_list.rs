use std::{fmt::Debug, marker::PhantomData, ops::{Deref, DerefMut}, ptr::NonNull, sync::{Arc, atomic::{AtomicBool, AtomicUsize, Ordering}}};

use crate::{CowData, MutCowData, RefCowData};

type Link<T> = CowData<Node<T>>;

/// Node of a shared linked list.
/// Contains a mutex to the stored data and next node link.
pub struct Node<T> {
    data: Option<T>,
    next: Link<T>
}

impl <T: Clone> Clone for Node<T> {
    fn clone(&self) -> Self {
        Self { data: self.data.clone(), next: self.next.clone() }
    }
}

/// The inner shared structure of a shared linked list.
/// Contains:
/// - should_swap_fn: An atomic pointer to an optional that 
///         may contain a should swap function for sorting nodes.
/// - head: An atomic pointer to an optional that may contain the
///         first node in the linked list.
/// - ptrs: An atomic counter of all active references to 
///         this shared object.
/// - len: An atomic counter of the number of nodes in this linked list.
pub struct SharedListInner<T> {
    order_fn: NonNull<Option<fn(&T, &T) -> std::cmp::Ordering>>,
    enforce_noneq: AtomicBool,
    head: Link<T>,
    len: AtomicUsize
}

impl<T> SharedListInner<T> {
    /// Creates a new empty shared linked list shared structure.
    pub fn new() -> Self {
        SharedListInner {
            order_fn: unsafe { NonNull::new_unchecked(Box::leak(Box::new(None))) },
            enforce_noneq: AtomicBool::new(false),
            head: CowData::null(),
            len: AtomicUsize::new(0)
        }
    }
    
    /// Creates a new empty shared linked list shared structure with ordering.
    pub fn new_ordered(func: fn(&T, &T) -> std::cmp::Ordering) -> Self {
        SharedListInner {
            order_fn: unsafe { NonNull::new_unchecked(Box::leak(Box::new(Some(func)))) },
            enforce_noneq: AtomicBool::new(false),
            head: CowData::null(),
            len: AtomicUsize::new(0)
        }
    }

    /// Sets the enforce non-equals flag to the given value.
    /// When enabled and a ordering function has been set, attempting to push a 
    /// new value onto the internal linked list that returns an equals ordering
    /// will result in the push failing silently.  This is meant to avoid duplicates
    /// when needed.
    pub fn set_enforce_noneq(&self, enforce_noneq: bool) {
        self.enforce_noneq.store(enforce_noneq, Ordering::Release);
    }

    /// Pushes the given data into the list.
    /// If no order function has been set, a node with 
    ///     the given data will be pushed to the start or head of the list.
    /// If a order function is set, the node will be placed in-between the first
    ///     neighbour pair of nodes where the first is less than the added data
    ///     and the second is greater than the added data.
    /// If an order function is set, the enforce non-equal flag is set to true,
    ///     and a node is found to have equal ordering to the given data, the
    ///     data will simply not be inserted.
    pub fn push(&self, data: T) {
        let should_set_head = {
            if self.head.is_null() { true }
            else {
                if let Some(order_fn) = unsafe { &*self.order_fn.as_ptr() } {
                    let guard = self.head.get_ref();
                    let enforce_noneq = self.enforce_noneq.load(Ordering::Acquire);
                    let vs_head = order_fn(guard.data.as_ref().unwrap(), &data);

                    if enforce_noneq && vs_head == std::cmp::Ordering::Equal { return }
                    let result = vs_head != std::cmp::Ordering::Greater;
                    result
                } else { true }
            }
        };

        if should_set_head {
            // create new node and insert
            let old_head_slot = CowData::null();
            let new_head = Node {
                data: Some(data),
                next: old_head_slot.clone()
            };

            self.head.bump_into_null(&old_head_slot, new_head);
        } else {
            // we know that head is something at this point
            let mut cur_node = self.head.clone();
            loop {
                if cur_node.is_null() { break }
                let next = cur_node.get_ref().next.clone();

                // check if we should insert in between current and its old next node
                let insert_here = {
                    if next.is_null() { true }
                    else {
                        if let Some(order_fn) = unsafe { &*self.order_fn.as_ptr() } {
                            let enforce_noneq = self.enforce_noneq.load(Ordering::Acquire);
                            let vs_head = order_fn((&*next.get_ref()).data.as_ref().unwrap(), &data);

                            if enforce_noneq && vs_head == std::cmp::Ordering::Equal { return }
                            vs_head == std::cmp::Ordering::Less
                        } else { true }
                    }
                };

                // if we should insert here or next is empty, stop loop
                if insert_here {
                    break;
                }

                // move forward
                cur_node = next;
            }

            // insert between current and currents old next
            let last = cur_node.get_ref().next.clone();

            let old_slot = CowData::null();
            let node = Node {
                data: Some(data),
                next: old_slot.clone()
            };

            // insert
            last.bump_into_null(&old_slot, node);
        }

        self.len.fetch_add(1, Ordering::Relaxed);
    }

    /// Searches through every element in this list, stopping and returning the first
    /// element that matches the given condition function.
    pub fn remove_search<F: FnMut(&T) -> bool>(&self, mut func: F) -> Option<T> {
        let mut current = self.head.clone();

        loop {
            // get current and check if this matches to_remove, stopping if current is empty
            if current.is_null() { break None }
            let matches = func(current.get_ref().data.as_ref().unwrap());

            // if matches, remove and return current, otherwise, move to next
            if matches {
                let old = current.swap(&current.get_ref().next);
                self.len.fetch_sub(1, Ordering::Relaxed);
                break old.map(|mut a| a.data.take()).flatten();
            } else {
                current = current.get_ref().next.clone();
            }
        }
    }

    /// Returns the length of this linked list using the internal length counter.
    pub fn len(&self) -> usize {
        self.len.load(Ordering::Acquire)
    }

    /// Returns to if the stored length in the internal length counter is 0.
    pub fn is_empty(&self) -> bool {
        self.len.load(Ordering::Acquire) == 0
    }

    /// Returns an iterator to the data in this list.
    /// All data will be wrapped in `SharedData` wrapper to allow
    /// infinite reference but controlled mutability.
    pub fn iter<'a>(&'a self) -> Iter<'a, T> {
        return Iter { 
            current: self.head.clone(),
            _phantom: PhantomData::default()
        };
    }

    /// Find data that matches a given condition.
    pub fn find<'a, F>(&'a self, mut func: F) -> Option<&'a T>
        where F: FnMut(&T) -> bool
    {
        let mut current = self.head.clone();
        loop {
            if current.is_null() { break None };

            if func(current.get_ref().data.as_ref().unwrap()) {
                let ref_cow_data = current.get_ref();

                // SAFETY: The reference is valid for the lifetime 'a because it's tied to the list
                // which holds the necessary data structures alive through CowData
                break unsafe {
                    std::mem::transmute(ref_cow_data.data.as_ref())
                }
            }

            current = current.get_ref().next.clone();
        }
    }

    /// Find data that matches a given condition and return its mapped value.
    pub fn find_map<'a, F, O>(&'a self, mut func: F) -> Option<&'a O>
        where F: FnMut(&T) -> Option<&'a O>
    {
        let mut current = self.head.clone();
        loop {
            if current.is_null() { break None };
            if let Some(data) = func(current.get_ref().data.as_ref().unwrap()) { break Some(data) }

            current = current.get_ref().next.clone();
        }
    }

    /// Removes all that meet the given condition function and returns them as a vector.
    pub fn remove_all<F>(&self, mut should_remove: F) -> Vec<T>
        where F: FnMut(&T) -> bool
    {
        let mut output = Vec::new();

        if self.head.is_null() { return output; }
        let mut current = self.head.clone();

        loop {
            // if current meets given condition, remove from linked list and add to output list, otherwise, move node forward
            if should_remove(current.get_ref().data.as_ref().unwrap()) {
                let old = current.swap(&current.get_ref().next);
                self.len.fetch_sub(1, Ordering::Relaxed);
                if let Some(mut old) = old { output.push(old.data.take().unwrap()); }
            } else {
                current = current.get_ref().next.clone();
            }

            // if next slot is empty, stop
            if current.is_null() { break };
        }

        return output;
    }
    
    /// Creates a draining iterator for a `SharedList`.  
    /// This iterator just calls `SharedList`s `pop` 
    /// function repeatedly until `pop` returns nothing.
    pub fn drain(&self) -> Drain<T> {
        let new_head = CowData::null();
        let old_head = self.head.swap(&new_head);
        let cow = if let Some(node) = old_head { CowData::new(node) } else { CowData::null() };
        self.len.store(0, Ordering::Release);

        Drain { list: cow }
    }

    pub fn extend<I>(&self, iter: I) 
        where I: IntoIterator<Item = T>
    {
        iter.into_iter().for_each(|item| self.push(item));

        // todo no way to do this O(1i) only found way was O(1o + 1i)
        // if unsafe { *self.order_fn.as_ptr() }.is_some() {
        //     iter.into_iter().for_each(|item| self.push(item));
        // } else {
        //     // get and create head
        //     let mut iter = iter.into_iter();
        //     let Some(head) = iter.next() else { return };

        //     // create current head tracker
        //     let tail_ptr = CowData::new(
        //         Node {
        //             data: Some(head),
        //             next: CowData::null()
        //         }
        //     );
        //     let current_head = tail_ptr.clone();

        //     loop {
        //         let Some(next) = iter.next() else { break };
                
        //         let node = Node {
        //             data: Some(next),
        //             next: CowData::null()
        //         };

        //         current_head.store(Box::leak(Box::new(node)), Ordering::Release);
        //     }

        //     // insert new node in the front of the list
        //     {
        //         tail_ptr.swap(&self.head);
        //         self.head.swap(&current_head);
        //     }
        // }
    }
    
    /// Removes the first element from this list if one exists in the
    /// head variable of this structure.  The new head will be the node
    /// referenced as "next" by the head.  Nothing will be returned if the
    /// head is empty.
    pub fn pop(&self) -> Option<T> {
        // if no head, default to empty responses
        let head = self.head.clone();
        if head.is_null() { return None };

        // subtract from length
        self.len.fetch_sub(1, Ordering::Relaxed);

        // swap new head to head next and return old head
        let next = &head.get_ref().next;
        let old = self.head.swap(&next);
        return old.map(|mut a| a.data.take()).flatten();
    }
}

impl <T: Clone> SharedListInner<T> {
    /// Returns a mutable iterator to the data in this list.
    /// All data will be wrapped in `SharedData` wrapper to allow
    /// infinite reference but controlled mutability.
    pub fn iter_mut(&self) -> IterMut<T> {
        return IterMut { 
            current: self.head.clone()
        };
    }
}

impl <T: PartialEq> SharedListInner<T> {
    /// Remove a specific element from the reference
    pub fn remove(&self, to_remove: &T) -> Option<T> {
        let mut current = self.head.clone();

        loop {
            // get current and check if this matches to_remove, stopping if current is empty
            if current.is_null() { break None }
            let matches = current.get_ref().data.as_ref().unwrap() == to_remove;

            // if matches, remove and return current, otherwise, move to next
            if matches {
                let next_ptr = current.get_ref().next.clone();
                let old = current.swap(&next_ptr);
                self.len.fetch_sub(1, Ordering::Relaxed);
                break old.map(|mut a| a.data.take()).flatten();
            } else {
                current = current.get_ref().next.clone();
            }
        }
    }

    /// Checks if this list contains a certain element.
    pub fn contains(&self, contains: &T) -> bool {
        let mut iter = self.iter();
        iter.any(|data| &*data == contains)
    }
}

impl <T> Drop for SharedListInner<T> {
    fn drop(&mut self) {
        // drop order function ptr
        let _fn_ptr = unsafe { 
            Box::from_raw(self.order_fn.as_ptr())
        };
    }
}

pub struct SharedList<T> {
    inner: Arc<SharedListInner<T>>
}

unsafe impl <T> Send for SharedList<T> {}
unsafe impl <T> Sync for SharedList<T> {}

impl <T> SharedList<T> {
    pub fn new() -> Self {
        Self { inner: Arc::new(SharedListInner::<T>::new()) }
    }

    pub fn new_ordered(func: fn(&T, &T) -> std::cmp::Ordering) -> Self {
        Self { inner: Arc::new(SharedListInner::<T>::new_ordered(func)) }
    }
}

impl <T> Default for SharedList<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl <T> Clone for SharedList<T> {
    fn clone(&self) -> Self {
        Self { inner: Arc::clone(&self.inner) }
    }
}

impl <T> Deref for SharedList<T> {
    type Target = SharedListInner<T>;
    fn deref(&self) -> &Self::Target {
        self.inner.as_ref()
    }
}

impl <T: Debug> Debug for SharedList<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut fmt = f.debug_list();
        let iter = self.iter();
        iter.for_each(|data| { fmt.entry(&*data); });
        fmt.finish()
    }
}

pub struct Iter<'a, T> {
    current: CowData<Node<T>>,
    _phantom: PhantomData<&'a ()>
}

impl <'a, T: 'a> Iterator for Iter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        // clone current and get its node, returning none if no node present
        if self.current.is_null() { return None };

        let ref_cow_data = self.current.get_ref();

        // move current forward
        self.current = self.current.get_ref().next.clone();

        // SAFETY: The reference is valid for the lifetime 'a because it's tied to the iterator
        // which holds the necessary data structures alive through CowData
        unsafe {
            std::mem::transmute(ref_cow_data.data.as_ref())
        }
    }
}

pub struct RefNodeData<T> {
    ref_cow_data: RefCowData<Node<T>>
}

impl <T> Deref for RefNodeData<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        self.ref_cow_data.data.as_ref().unwrap()
    }
}

pub struct IterMut<T: Clone> {
    current: CowData<Node<T>>
}

impl <T: Clone> Iterator for IterMut<T> {
    type Item = MutCowData<Node<T>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current.is_null() { return None };

        let ref_cow_data = self.current.clone().get_mut();

        // move current forward
        self.current = self.current.get_ref().next.clone();

        // Some(MutNodeData { ref_cow_data })
        Some(ref_cow_data)
    }
}

pub struct MutNodeData<T: Clone> {
    ref_cow_data: MutCowData<Node<T>>
}

impl <T: Clone> Deref for MutNodeData<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        self.ref_cow_data.data.as_ref().unwrap()
    }
}

impl <T: Clone> DerefMut for MutNodeData<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.ref_cow_data.deref_mut().data.as_mut().unwrap()
    }
}

/// Draining iterator for a `SharedList`.  This iterator
/// just calls `SharedList`s `pop` function repeatedly
/// until `pop` returns nothing.
pub struct Drain<T> {
    list: CowData<Node<T>>
}

impl <T> Iterator for Drain<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.list.is_null() { return None }
        let old = self.list.swap(&self.list.get_ref().next);
        if let Some(mut old) = old { old.data.take() } else { None }
    }
}

#[cfg(test)]
mod tests {
    use crate::shared::SharedList;

    // #[test]
    // pub fn ml_pop_test() {
    //     let thread_count = 8;
    //     let mut threads = Vec::new();

    //     let mut list = SharedList::new();
    //     list.extend([0, 1, 2, 3, 4, 5]);

    //     for _ in 0 .. thread_count {
    //         let list = list.clone();
    //         let handle = std::thread::spawn(move || {
    //             let _ = list.pop();
    //         });

    //         threads.push(handle);
    //     }

    //     std::thread::sleep(std::time::Duration::from_secs_f32(30.0));
    
    //     threads.into_iter().for_each(|thread| thread.join().unwrap());
    // }

    #[test]
    pub fn ml_test() {
        let list = SharedList::<i32>::new();

        {
            let list2 = SharedList::clone(&list);
            let thread = std::thread::spawn(move || {
                list2.push(123);
                list2.push(456);
            });
            thread.join().unwrap();
        }

        let mut iter = list.iter();
        assert!(**iter.next().as_ref().unwrap() == 456);
        assert!(**iter.next().as_ref().unwrap() == 123);
    }

    #[test]
    pub fn insert_test_one() {
        let list = SharedList::<i32>::new();
        list.push(123);
    
        assert!(list.len() == 1);
        let mut iter = list.iter();
    
        assert!(**iter.next().as_ref().unwrap() == 123);
    }

    #[test]
    pub fn insert_test_two() {
        let list = SharedList::<i32>::new();
        list.push(123);
        list.push(256);

        assert!(list.len() == 2);
        let mut iter = list.iter();

        assert!(**iter.next().as_ref().unwrap() == 256);
        assert!(**iter.next().as_ref().unwrap() == 123);
    }

    #[test]
    pub fn insert_test_three() {
        let list = SharedList::<i32>::new();
        list.push(123);
        list.push(256);
        list.push(8657);
        let _ = list.pop();

        assert!(list.len() == 2);
        
        let mut iter = list.iter();
        assert!(**iter.next().as_ref().unwrap() == 256);
        assert!(**iter.next().as_ref().unwrap() == 123);
    }

    #[test]
    pub fn insert_test_four() {
        let list = SharedList::<i32>::new();
        list.push(123);
        list.push(256);
        list.push(8657);
        list.pop();
        list.pop();
        list.pop();

        assert!(list.len() == 0);
    }

    #[test]
    pub fn drain_test_one() {
        let list = SharedList::<i32>::new();
        list.push(123);
        list.push(256);
        list.push(8657);

        let mut drain = list.drain();

        assert!(**drain.next().as_ref().unwrap() == 8657);
        assert!(**drain.next().as_ref().unwrap() == 256);
        assert!(**drain.next().as_ref().unwrap() == 123);
    }

    #[test]
    pub fn sorted_insert_test_one() {
        let list = SharedList::<i32>::new_ordered(|a, b| b.cmp(a));
        list.push(256);
        list.push(123);

        assert!(list.len() == 2);

        let mut iter = list.iter();
        assert!(**iter.next().as_ref().unwrap() == 123);
        assert!(**iter.next().as_ref().unwrap() == 256);
    }

    #[test]
    pub fn sorted_insert_test_two() {
        let list = SharedList::<i32>::new_ordered(|a, b| b.cmp(a));
        list.push(123);
        list.push(256);

        assert!(list.len() == 2);

        let mut iter = list.iter();
        assert!(**iter.next().as_ref().unwrap() == 123);
        assert!(**iter.next().as_ref().unwrap() == 256);
    }

    #[test]
    pub fn sorted_insert_test_three() {
        let list = SharedList::<i32>::new_ordered(|a, b| b.cmp(a));
        list.push(123);
        list.push(256);
        list.push(200);

        assert!(list.len() == 3);

        let mut iter = list.iter();
        assert!(**iter.next().as_ref().unwrap() == 123);
        assert!(**iter.next().as_ref().unwrap() == 200);
        assert!(**iter.next().as_ref().unwrap() == 256);
    }

    #[test]
    pub fn sorted_insert_test_four() {
        let list = SharedList::<i32>::new_ordered(|a, b| b.cmp(a));
        list.push(123);
        list.push(256);
        list.push(200);
        list.push(100);
        list.push(180);
        list.push(190);

        assert!(list.len() == 6);

        let mut iter = list.iter();
        assert!(**iter.next().as_ref().unwrap() == 100);
        assert!(**iter.next().as_ref().unwrap() == 123);
        assert!(**iter.next().as_ref().unwrap() == 180);
        assert!(**iter.next().as_ref().unwrap() == 190);
        assert!(**iter.next().as_ref().unwrap() == 200);
        assert!(**iter.next().as_ref().unwrap() == 256);
    }

    #[test]
    pub fn sorted_insert_test_five() {
        let list = SharedList::<i32>::new_ordered(|a, b| b.cmp(a));
        list.push(123);
        list.push(256);
        list.push(200);
        list.push(100);
        list.push(180);
        list.push(190);
        assert!(list.contains(&200));
        list.remove(&200);
        list.remove(&180);

        assert!(list.len() == 4);

        let mut iter = list.iter();
        assert!(**iter.next().as_ref().unwrap() == 100);
        assert!(**iter.next().as_ref().unwrap() == 123);
        assert!(**iter.next().as_ref().unwrap() == 190);
        assert!(**iter.next().as_ref().unwrap() == 256);
    }

    #[test]
    pub fn sorted_test_noneq() {
        let list = SharedList::<i32>::new_ordered(|a, b| b.cmp(a));
        list.set_enforce_noneq(true);
        list.push(43);
        list.push(43);
        list.push(43);
        
        assert!(list.len() == 1);

        let mut iter = list.iter();
        assert!(**iter.next().as_ref().unwrap() == 43);
    }

    #[test]
    pub fn sorted_test_noneq_two() {
        let list = SharedList::<i32>::new_ordered(|a, b| b.cmp(a));
        list.set_enforce_noneq(true);
        list.push(23);
        list.push(43);
        list.push(43);
        list.push(43);
        
        assert!(list.len() == 2);

        let mut iter = list.iter();
        assert!(**iter.next().as_ref().unwrap() == 23);
        assert!(**iter.next().as_ref().unwrap() == 43);
    }

    #[test]
    pub fn extend_test_one() {
        let list = SharedList::<i32>::new_ordered(|a, b| b.cmp(a));
        list.extend([3_i32, 2_i32, 1_i32]);

        let mut iter = list.iter();
        assert!(**iter.next().as_ref().unwrap() == 1);
        assert!(**iter.next().as_ref().unwrap() == 2);
        assert!(**iter.next().as_ref().unwrap() == 3);
    }

    #[test]
    pub fn extend_test_two() {
        let list = SharedList::<i32>::new_ordered(|a, b| b.cmp(a));
        list.push(3);
        list.extend([4_i32, 2_i32, 1_i32]);

        let mut iter = list.iter();
        assert!(**iter.next().as_ref().unwrap() == 1);
        assert!(**iter.next().as_ref().unwrap() == 2);
        assert!(**iter.next().as_ref().unwrap() == 3);
        assert!(**iter.next().as_ref().unwrap() == 4);
    }

    #[test]
    pub fn extend_test_three() {
        let list = SharedList::<i32>::new();
        list.extend([1_i32, 2_i32, 3_i32]);

        // Note: Without ordering, extend inserts at the front backwards.

        let mut iter = list.iter();
        assert!(**iter.next().as_ref().unwrap() == 3);
        assert!(**iter.next().as_ref().unwrap() == 2);
        assert!(**iter.next().as_ref().unwrap() == 1);
    }

    #[test]
    pub fn extend_test_four() {
        let list = SharedList::<i32>::new();
        list.push(3);
        list.extend([4_i32, 2_i32, 1_i32]);

        // Note: Without ordering, extend inserts at the front backwards.

        let mut iter = list.iter();
        assert!(**iter.next().as_ref().unwrap() == 1);
        assert!(**iter.next().as_ref().unwrap() == 2);
        assert!(**iter.next().as_ref().unwrap() == 4);
        assert!(**iter.next().as_ref().unwrap() == 3);
    }
}
