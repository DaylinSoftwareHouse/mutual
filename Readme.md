# Mutual
A useful library of complex atomic objects for maps, sets, lists and COWs.  The a utility library
used by a work-in-progress game engine to allow for a higher level of concurrency in a shared
game world.

## Goal
Provide easy to use and thread-safe data structures that can be used to share information between
threads safetly, cheaply, and easily.  This package also provides reexports and/or wrappers of 
preexisting implementations of useful data structures like `DashMap` or `ArcSwap` wrapped in `CowData`.

## Type: BitSet
A simple bit set.  This is not a shareable thread-safe data structure, just a useful tool for building
bit sets as a list of bytes.

```rust
let mut set = BitSet::new();
set.insert(3);
set.insert_slice(&[23, 1]);

set.remove(3);
```

## Type: CowData
A simple wrapper around `ArcSwap` that allows threads to maintain immutable access to a piece of data
while other threads may write to a copy of the data that will only be provided to future reads after
the write is complete.

```rust
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
```

## Type: RelaxedMutex
An data structure to a mutex that allows for an infinite amount of immutable reads to occur, while 
only blocking if a mutable read is requested or is currently active.

```rust
let mutex = RelaxedMutex::new(3);
{
    assert!(*mutex.lock_ref() == 3);
} // has to be in its own block so that the guard provided by lock_ref is dropped before the write.
{
    *mutex.lock_mut = 2;
} // has to be in its own block so that the guard provided by lock_mut is dropped before the next read.
assert!(*mutex.lock_ref() == 2);
```

## Type: SharedList
A simple singely linked list that allows users to created sorted or unsorted lists that any thread
can read, write, iterate over, or drain from at the same time.

```rust
let list = SharedList::<i32>::new();
list.push(123);
list.push(256);

assert!(list.len() == 2);
let mut iter = list.iter();

assert!(**iter.next().as_ref().unwrap() == 256);
assert!(**iter.next().as_ref().unwrap() == 123);
```

## Type: SharedMap
A simple 16 bucket hash map.  This is a lighter-weight alternative to `DashMap` but may not be the best
option for performance with large amounts of objects.

```rust
let map = SharedMap::<i32, i32>::new();
map.insert(32, 74563);
map.insert(45, 984023);

assert!(map.contains(&45));
assert!(!map.contains(&54));
assert!(*map.get(&45).unwrap() == 984023);
assert!(*map.remove(&32).unwrap() == 74563);
```

## Type: SharedSet
A simple wrapper around `SharedMap` to allow for set like operations.  While this is a lighter-weight
operation than `DashSet`, it would be better to use `DashSet` in 99% of cases.

```rust
let map = SharedSet::<i32>::new();
map.insert(32);
map.insert(45);

assert!(map.contains(&45));
assert!(!map.contains(&54));
```

## Reexport: ArcSwap
A useful data cow like data structure that allows a thread to write to a piece of data while other
threads can keep the old copy to read from simultaneously.

## Reexport: DashMap
An amazing Rust implementation of Javas ConcurrentHashMap.  Roughly 99.99% of the time it would be 
better to use this than `SharedMap`.

## Reexport: DashSet
An amazing Rust implementation of Javas ConcurrentHashMap wrapped to become a set.  Roughly 99.99% 
of the time it would be better to use this than `SharedSet`.

## Reexport: Siphasher
A hasher that I use a lot and is used in this package.
