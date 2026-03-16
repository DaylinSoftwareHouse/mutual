use std::collections::LinkedList;

use serde::{Deserialize, Serialize};

/// A useful tool for creating bit sets in a way that is stored
/// as a linked list of bytes.
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct BitSet {
    internal: LinkedList<u8>
}

impl BitSet {
    /// Creates a new `BitSet`.
    pub fn new() -> Self { Self::default() }

    /// Inserts a certain bit in the bit set.
    pub fn insert(&mut self, idx: u32) {
        let segment_idx = (idx / 8) as usize;
        let sub_idx = (idx % 8) as usize;

        let segment = self.internal
            .iter_mut()
            .skip(segment_idx)
            .next();

        if let Some(segment) = segment {
            *segment |= 1 << sub_idx;
        } else {
            let len_diff = segment_idx - self.internal.len();
            if len_diff > 0 {
                (0 .. len_diff).for_each(|_| self.internal.push_back(0_u8));
            }
            self.internal.push_back(1 << sub_idx);
        }
    }

    /// Inserts all of an iterator into this bit set.
    pub fn insert_all<I: Iterator<Item = u32>>(&mut self, iter: I) {
        iter.for_each(|item| self.insert(item));
    }

    /// Inserts all of a slice into the bit set.
    pub fn insert_slice(&mut self, slice: &[u32]) {
        self.insert_all(slice.iter().map(|a| *a));
    }

    /// Removes a certain bit from the bit set.
    pub fn remove(&mut self, idx: u32) {
        let segment_idx = (idx / 8) as usize;

        let Some(segment) = self.internal
            .iter_mut()
            .skip(segment_idx)
            .next() else { return };

        // get inverted mask to remove the bit
        let sub_idx = (idx % 8) as usize;
        let mask = !(1_u8 << sub_idx);
        *segment &= mask;
    }

    /// Checks if the given index has been set in this bit set.
    pub fn contains(&self, idx: u32) -> bool {
        let segment_idx = (idx / 8) as usize;
        let sub_idx = (idx % 8) as usize;

        self.internal
            .iter()
            .skip(segment_idx)
            .next()
            .map(|segment| segment & 1 << sub_idx > 0)
            .unwrap_or(false)
    }

    /// Returns true if this `BitSet` matches the other `BitSet`.
    pub fn matches(&self, other: &Self) -> bool {
        // make sure self is the largest bit set
        if self.internal.len() < other.internal.len() {
            return other.matches(self)
        }

        // get iterators
        let mut self_iter = self.internal.iter();
        let mut other_iter = other.internal.iter();

        loop {
            // get next self segment, if out of elements, everything matched
            let Some(self_seg) = self_iter.next() else { return true };

            // get other segment, if none found, do the following:
            //  - if self segment has bits set, return no match
            //  - otherwise, skip to next iteration
            let Some(other_seg) = other_iter.next() else {
                if *self_seg > 0 { return false }
                else { continue }
            };

            // check if self and other segment match, if they dont, return no match
            if self_seg != other_seg { return false }
        }
    }

    /// Returns true if self is a subset of the given super set.
    pub fn is_subset(&self, super_set: &Self) -> bool {
        let mut sub_iter = self.internal.iter();
        let mut super_iter = super_set.internal.iter();

        loop {
            let mut empty_count = 0;
            let super_seg = super_iter.next().map(|a| *a).unwrap_or_else(|| { empty_count += 1; 0 });
            let sub_seg = sub_iter.next().map(|a| *a).unwrap_or_else(|| { empty_count += 2; 0 });

            if empty_count >= 2 { return true }
            else if empty_count == 1 && sub_seg < 1 { return false }

            if super_seg & sub_seg != sub_seg { return false }
        }
    }

    /// Returns true if self is a super set of the given subset.
    pub fn is_superset(&self, sub_set: &Self) -> bool { sub_set.is_subset(self) }
}

impl bincode::Encode for BitSet {
    fn encode<E: bincode::enc::Encoder>(&self, encoder: &mut E) -> Result<(), bincode::error::EncodeError> {
        let vec = self.internal.clone().into_iter().collect::<Vec<_>>();
        bincode::Encode::encode(&vec, encoder)?;
        Ok(())
    }
}

impl <Context> bincode::Decode<Context> for BitSet {
    fn decode<D: bincode::de::Decoder>(decoder: &mut D) -> Result<Self, bincode::error::DecodeError> {
        let vec: Vec<u8> = bincode::Decode::decode(decoder)?;
        Ok(Self { internal: LinkedList::from_iter(vec.into_iter()) })
    }
}

impl <'de, Context> bincode::de::BorrowDecode<'de, Context> for BitSet {
    fn borrow_decode<D: bincode::de::BorrowDecoder<'de>>(
        decoder: &mut D,
    ) -> Result<Self, bincode::error::DecodeError> {
        let vec: Vec<u8> = bincode::Decode::decode(decoder)?;
        Ok(Self { internal: LinkedList::from_iter(vec.into_iter()) })
    }
}

#[cfg(test)]
mod tests {
    use crate::BitSet;

    #[test]
    pub fn test_bit_set_insert() {
        let mut set = BitSet::new();
        set.insert(3);
        set.insert(23);
        set.insert(1);

        assert!(set.contains(23) == true);
        assert!(set.contains(3) == true);
        assert!(set.contains(1) == true);
        assert!(set.contains(2) == false);
        assert!(set.contains(61) == false);
        assert!(set.contains(37) == false);
    }

    #[test]
    pub fn test_bit_set_remove() {
        let mut set = BitSet::new();
        set.insert(3);
        set.insert(23);
        set.insert(1);

        set.remove(3);

        assert!(set.contains(23) == true);
        assert!(set.contains(3) == false);
        assert!(set.contains(1) == true);
        assert!(set.contains(2) == false);
        assert!(set.contains(61) == false);
        assert!(set.contains(37) == false);
    }

    #[test]
    pub fn test_bit_set_bulk_insert() {
        let mut set = BitSet::new();
        set.insert_slice(&[4, 23, 41, 1, 8]);

        assert!(set.contains(23) == true);
        assert!(set.contains(4) == true);
        assert!(set.contains(1) == true);
        assert!(set.contains(8) == true);
        assert!(set.contains(41) == true);
        assert!(set.contains(37) == false);
    }

    #[test]
    pub fn test_bit_set_match() {
        let mut set = BitSet::new();
        set.insert_slice(&[4, 23, 41, 1, 8]);

        let set2 = set.clone();
        assert!(set.matches(&set2) == true);
    }

    #[test]
    pub fn test_bit_set_match_two() {
        let mut set = BitSet::new();
        set.insert_slice(&[4, 23, 41, 1, 8]);

        let mut set2 = set.clone();
        set2.remove(41);
        assert!(set.matches(&set2) == false);
    }

    #[test]
    pub fn test_bit_set_subset() {
        let mut set = BitSet::new();
        set.insert_slice(&[4, 23, 41, 1, 8]);

        let mut set2 = set.clone();
        set2.remove(8);
        assert!(set2.is_subset(&set) == true);
    }

    #[test]
    pub fn test_bit_set_subset_two() {
        let mut set = BitSet::new();
        set.insert_slice(&[4, 23, 41, 1, 8]);

        let mut set2 = set.clone();
        set2.remove(41);
        assert!(set2.is_subset(&set) == true);
    }

    #[test]
    pub fn test_bit_set_subset_three() {
        let mut set = BitSet::new();
        set.insert_slice(&[4, 23, 41, 1, 8]);

        let mut set2 = set.clone();
        set2.remove(1);
        assert!(set2.is_subset(&set) == true);
    }

    #[test]
    pub fn test_bit_set_subset_fail() {
        let mut set = BitSet::new();
        set.insert_slice(&[4, 23, 41, 1, 8]);

        let mut set2 = set.clone();
        set2.remove(8);
        set2.insert(36);
        assert!(set2.is_subset(&set) == false);
    }

    #[test]
    pub fn test_bit_set_subset_fail_two() {
        let mut set = BitSet::new();
        set.insert_slice(&[4, 23, 41, 1, 8]);

        let mut set2 = set.clone();
        set2.insert(36);
        assert!(set2.is_subset(&set) == false);
    }
}
