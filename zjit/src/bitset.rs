type Entry = u128;

// TODO(max): Make a `SmallBitSet` and `LargeBitSet` and switch between them if `num_bits` fits in
// `Entry`.
pub struct BitSet<T: Into<usize> + Copy> {
    storage: Vec<Entry>,
    num_bits: usize,
    phantom: std::marker::PhantomData<T>,
}

impl<T: Into<usize> + Copy> BitSet<T> {
    pub fn with_capacity(num_bits: usize) -> Self {
        // +1 because we are rounding down
        let num_entries = num_bits / (Entry::BITS as usize) + 1;
        Self { storage: vec![0; num_entries], num_bits, phantom: Default::default() }
    }

    /// Returns whether the value was newly inserted: true if the set did not originally contain
    /// the bit, and false otherwise.
    pub fn insert(&mut self, idx: T) -> bool {
        debug_assert!(idx.into() < self.num_bits);
        let entry_idx = idx.into() / (Entry::BITS as usize);
        let bit_idx = idx.into() % (Entry::BITS as usize);
        let newly_inserted = (self.storage[entry_idx] & (1 << bit_idx)) == 0;
        self.storage[entry_idx] |= 1 << bit_idx;
        newly_inserted
    }

    pub fn get(&self, idx: T) -> bool {
        debug_assert!(idx.into() < self.num_bits);
        let entry_idx = idx.into() / (Entry::BITS as usize);
        let bit_idx = idx.into() % (Entry::BITS as usize);
        (self.storage[entry_idx] & (1 << bit_idx)) != 0
    }

    /// Modify `self` to only have bits set if they are also set in `other`. Returns true if `self`
    /// was modified, and false otherwise.
    /// `self` and `other` must have the same number of bits.
    pub fn intersect_with(&mut self, other: &Self) -> bool {
        assert_eq!(self.num_bits, other.num_bits);
        let mut changed = false;
        for i in 0..self.storage.len() {
            let before = self.storage[i];
            self.storage[i] &= other.storage[i];
            changed |= self.storage[i] != before;
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::BitSet;

    #[test]
    #[should_panic]
    fn get_over_capacity_panics() {
        let set = BitSet::with_capacity(0);
        assert_eq!(set.get(0usize), false);
    }

    #[test]
    fn with_capacity_defaults_to_zero() {
        let set = BitSet::with_capacity(4);
        assert_eq!(set.get(0usize), false);
        assert_eq!(set.get(1usize), false);
        assert_eq!(set.get(2usize), false);
        assert_eq!(set.get(3usize), false);
    }

    #[test]
    fn insert_sets_bit() {
        let mut set = BitSet::with_capacity(4);
        assert_eq!(set.insert(1usize), true);
        assert_eq!(set.get(1usize), true);
    }

    #[test]
    fn insert_with_set_bit_returns_false() {
        let mut set = BitSet::with_capacity(4);
        assert_eq!(set.insert(1usize), true);
        assert_eq!(set.insert(1usize), false);
    }

    #[test]
    #[should_panic]
    fn intersect_with_panics_with_different_num_bits() {
        let mut left: BitSet<usize> = BitSet::with_capacity(3);
        let right = BitSet::with_capacity(4);
        left.intersect_with(&right);
    }

    #[test]
    fn intersect_with_keeps_only_common_bits() {
        let mut left = BitSet::with_capacity(3);
        let mut right = BitSet::with_capacity(3);
        left.insert(0usize);
        left.insert(1usize);
        right.insert(1usize);
        right.insert(2usize);
        left.intersect_with(&right);
        assert_eq!(left.get(0usize), false);
        assert_eq!(left.get(1usize), true);
        assert_eq!(left.get(2usize), false);
    }
}
