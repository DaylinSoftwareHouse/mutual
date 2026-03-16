use std::{fmt::Debug, sync::{Arc, atomic::{AtomicBool, AtomicUsize, Ordering}}};

use derive_more::{Deref, DerefMut};

use crate::{CowData, Mut, Ref, RefCowData};

type Link<T> = CowData<Node<T>>;

#[derive(Deref, DerefMut)]
pub struct Node<T> {
    #[deref] #[deref_mut]
    data: T,
    next: Link<T>
}

impl <T: Clone> Clone for Node<T> {
    fn clone(&self) -> Self {
        Node { data: self.data.clone(), next: self.next.clone() }
    }
}

impl <T: Debug> Debug for Node<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.data.fmt(f)
    }
}

/// A linked list implementation that is designed to be
/// shared across threads.  Clone will create a new structure
/// with the same contained data.
pub struct SharedList<T> {
    order_fn: CowData<fn(&T, &T) -> std::cmp::Ordering>,
    head: Link<T>,
    enforce_noneq: Arc<AtomicBool>,
    len: Arc<AtomicUsize>
}

impl <T: 'static> SharedList<T> {
    /// Creates a new empty shared linked list shared structure.
    pub fn new() -> Self {
        Self { 
            order_fn: CowData::null(), 
            head: CowData::null_lock_protected(), 
            enforce_noneq: Arc::new(AtomicBool::new(false)), 
            len: Arc::new(AtomicUsize::new(0)) 
        }
    }

    /// Creates a new empty shared linked list shared structure with ordering.
    pub fn new_ordered(func: fn(&T, &T) -> std::cmp::Ordering) -> Self {
        Self {
            order_fn: CowData::new(func),
            head: CowData::null_lock_protected(),
            enforce_noneq: Arc::new(AtomicBool::new(false)), 
            len: Arc::new(AtomicUsize::new(0)) 
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
    /// If an order function is set, the node will be placed in-between the first
    ///     neighbour pair of nodes where the first is less than the added data
    ///     and the second is greater than the added data.
    /// If an order function is set, the enforce non-equal flag is set to true,
    ///     and a node is found to have equal ordering to the given data, the
    ///     data will simply not be inserted.
    pub fn push(&self, data: T) {
        if self.order_fn.is_null() || self.head.is_null() {
            self.head.create_lock();

            if self.head.is_null() {
                let new_head = Node {
                    data,
                    next: CowData::null_lock_protected()
                };

                self.head.get_arc_swap().store(Arc::new(Some(new_head)));
                self.head.remove_lock();
            } else {
                let old_head_arc = (*self.head.get_arc_swap().load()).clone();

                let new_head = Node {
                    data,
                    next: CowData::from_arc_lock_protected(old_head_arc)
                };

                self.head.get_arc_swap().store(Arc::new(Some(new_head)));
                self.head.remove_lock();
            }
        } else {
            let mut data = Some(data);
            let order_fn = self.order_fn.get_ref();
            self.head.create_lock();
            let mut current = self.head.clone();

            let inserted = loop {
                // if no node, unlock and return no insertion complete
                if current.is_null() { 
                    break false 
                }

                // check if should insert here
                let node = current.get_ref();

                // if we should insert ahead of node, insert now
                let enforce_noneq = self.enforce_noneq.load(Ordering::Acquire);
                let vs_head = order_fn(&node.data, data.as_ref().unwrap());
                if enforce_noneq && vs_head == std::cmp::Ordering::Equal { current.remove_lock(); return }
                if vs_head == std::cmp::Ordering::Less {
                    let current_ptr = (*current.get_arc_swap().load()).clone();
                    let current2 = CowData::from_arc_lock_protected(current_ptr);

                    let new_node = Node {
                        data: data.take().unwrap(),
                        next: current2
                    };

                    current.get_arc_swap().store(Arc::new(Some(new_node)));
                    current.remove_lock();
                    break true
                }

                // move forward and unlock the node we are leaving
                current.remove_lock();
                current = node.next.clone();
            };

            // if no insertion, insert at the end, and then remove lock
            if !inserted {
                current.set(Node { data: data.take().unwrap(), next: CowData::null_lock_protected() });
                current.remove_lock(); 
            }
        }

        self.len.fetch_add(1, Ordering::AcqRel);
    }

    /// Removes the first element from this list if one exists in the
    /// head variable of this structure.  The new head will be the node
    /// referenced as "next" by the head.  Nothing will be returned if the
    /// head is empty.
    pub fn pop(&self) -> Option<Ref<T>> {
        self.head.create_lock();

        if self.head.is_null() { self.head.remove_lock(); return None }

        let old_head = self.head.get_ref();
        let next_ptr = Arc::clone(&*old_head.next.get_arc_swap().load());

        self.len.fetch_sub(1, Ordering::AcqRel);

        // if let Some(next_ptr) = next_ptr { self.head.get_multiarc().set_arc(next_ptr); }
        // else { self.head.get_multiarc().set_ptr(std::ptr::null_mut()); }
        self.head.get_arc_swap().store(next_ptr);
        self.head.remove_lock();

        return Some(Ref::new(
            old_head,
            |any| &any.downcast_ref::<RefCowData<Node<T>>>().unwrap().data
        ));
    }

    /// Extends this list with the given iterator.
    pub fn extend<I>(&self, iter: I) 
        where I: IntoIterator<Item = T>
    {
        for item in iter {
            self.push(item);
        }
    }
    
    /// Searches through every element in this list, stopping and returning the first
    /// element that matches the given condition function.
    pub fn remove_search<F>(&self, func: F) -> Option<Ref<T>> 
        where F: FnMut(&T) -> bool
    {
        let mut found = self.remove_count(func, 1);
        if found.is_empty() { return None }
        return Some(found.remove(0));
    }

    /// Removes all that meet the given condition function and returns them as a vector.
    pub fn remove_all<F>(&self, func: F) -> Vec<Ref<T>>
        where F: FnMut(&T) -> bool
    { self.remove_count(func, u32::MAX) }

    /// Attempts to remove the given count of items that meet the given function as a condition.
    /// The number of entries in the resulting vector may not meet the count if not enough elements are found.
    pub fn remove_count<F>(&self, mut func: F, mut count: u32) -> Vec<Ref<T>>
        where F: FnMut(&T) -> bool
    {
        let mut output = Vec::new();
        let mut current = self.head.clone();
        current.create_lock();

        while !current.is_null() && count > 0 {
            // read node and then drop the lock
            let node = current.get_ref();

            // if node matches according to func, return the node
            if func(&**node) { 
                let next_ptr = Arc::clone(&*node.next.get_arc_swap().load());
                output.push(Ref::new(node, |node| &node.downcast_ref::<RefCowData<Node<T>>>().unwrap().data));

                current.get_arc_swap().store(next_ptr);

                count -= 1;
                self.len.fetch_sub(1, Ordering::Release);
            } else {
                current.remove_lock();
                current = node.next.clone();
                current.create_lock();
            }
        }

        current.remove_lock();
        return output;
    }

    /// Find data that matches a given condition.
    pub fn find<F>(&self, mut func: F) -> Option<Ref<T>>
        where F: FnMut(&T) -> bool
    {
        let mut current = self.head.clone();

        while !current.is_null() {
            // read node and then drop the lock
            let node = current.get_ref();

            // if node matches according to func, return the node
            if func(&**node) { return Some(Ref::new(node, |node| &node.downcast_ref::<RefCowData<Node<T>>>().unwrap().data)); }
            current = node.next.clone();
        }

        return None;
    }

    /// Find data that matches a given condition and return its mapped value.
    pub fn find_map<F, O>(&self, mut func: F) -> Option<O>
        where F: FnMut(&T) -> Option<O>
    {
        let mut current = self.head.clone();

        while !current.is_null() {
            // read node and then drop the lock
            let node = current.get_ref();

            // if node matches according to func, return the node
            let opt = func(&**node);
            if opt.is_some() { return opt; }
            current = node.next.clone();
        }

        return None;
    }

    /// Returns an iterator to the data in this list.
    pub fn iter(&self) -> Iter<T> { 
        Iter { current: self.head.clone() } 
    }

    /// Creates a draining iterator for a `SharedList`.  
    /// This iterator just calls `SharedList`s `pop` 
    /// function repeatedly until `pop` returns nothing.
    pub fn drain(&self) -> Drain<T> {
        Drain { list: Self::clone(self) }
    }

    /// Returns the length of this linked list using the internal length counter.
    pub fn len(&self) -> usize {
        self.len.load(Ordering::Acquire)
    }

    /// Returns to if the stored length in the internal length counter is 0.
    pub fn is_empty(&self) -> bool {
        self.len.load(Ordering::Acquire) == 0
    }
}

impl <T> Clone for SharedList<T> {
    fn clone(&self) -> Self {
        Self {
            order_fn: self.order_fn.clone(),
            head: self.head.clone(),
            enforce_noneq: self.enforce_noneq.clone(),
            len: self.len.clone()
        }
    }
}

impl <T: 'static> Default for SharedList<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl <T: Clone> SharedList<T> {
    /// Returns a mutable iterator to the data in this list.
    /// All data will be wrapped in `MutCowData` wrapper to allow
    /// infinite reference but controlled mutability.
    pub fn iter_mut(&self) -> IterMut<T> {
        IterMut { current: self.head.clone() }
    }
}

impl <T: PartialEq + 'static> SharedList<T> {
    /// Checks if this list contains a certain element.
    pub fn contains(&self, contains: &T) -> bool {
        let mut current = self.head.clone();

        // loop through the list until the end is reached, checking each element
        while !current.is_null() {
            let node = current.get_ref();

            // if the current nodes data and contains match, return true
            if &node.data == contains { return true }

            // move tracker forward
            current = node.next.clone();
        }

        return false;
    }

    /// Remove a specific element from the reference
    pub fn remove(&self, to_remove: &T) -> Option<Ref<T>> {
        self.remove_search(|entry| entry == to_remove)
    }
}

pub struct Iter<T> {
    current: CowData<Node<T>>
}

impl <T: 'static> Iterator for Iter<T> {
    type Item = Ref<T>;

    fn next(&mut self) -> Option<Self::Item> {
        // clone current and get its node, returning none if no node present
        if self.current.is_null() { return None };

        let ref_cow_data = self.current.get_ref();

        // move current forward
        self.current = ref_cow_data.next.clone();

        Some(Ref::new(
            ref_cow_data,
            |any| &any.downcast_ref::<RefCowData<Node<T>>>().unwrap().data
        ))
    }
}

pub struct IterMut<T: Clone> {
    current: CowData<Node<T>>
}

impl <T: Clone + 'static> Iterator for IterMut<T> {
    type Item = Mut<T>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current.is_null() { return None };

        let ref_cow_data = self.current.clone().get_mut();

        // move current forward
        self.current = self.current.get_ref().next.clone();

        // Some(MutNodeData { ref_cow_data })
        Some(Mut::new(
            ref_cow_data,
            |any| &any.data,
            |any| &mut any.data
        ))
    }
}

pub struct Drain<T> {
    list: SharedList<T>
}

impl <T: 'static> Iterator for Drain<T> {
    type Item = Ref<T>;

    fn next(&mut self) -> Option<Self::Item> {
        self.list.pop()
    }
}

#[cfg(test)]
mod tests {
    use crate::SharedList;

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
    pub fn unsorted_insert_test_one() {
        let list = SharedList::<i32>::new();
        list.push(123);
    
        assert!(list.len() == 1);
        let mut iter = list.iter();
    
        assert!(**iter.next().as_ref().unwrap() == 123);
    }

    #[test]
    pub fn unsorted_insert_test_two() {
        let list = SharedList::<i32>::new();
        list.push(123);
        list.push(256);

        assert!(list.len() == 2);
        let mut iter = list.iter();

        assert!(**iter.next().as_ref().unwrap() == 256);
        assert!(**iter.next().as_ref().unwrap() == 123);
    }

    #[test]
    pub fn unsorted_insert_test_three() {
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
    pub fn unsorted_insert_test_four() {
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
