#[derive(Debug, Clone)]
pub struct Distribution<T: Copy + PartialEq + Default, const N: usize> {
    /// buckets and counts have the same length
    buckets: [T; N],
    counts: [usize; N],
    /// if there is no more room, increment the fallback
    other: usize,
}

impl<T: Copy + PartialEq + Default, const N: usize> Distribution<T, N> {
    pub fn new() -> Self {
        Self { buckets: [Default::default(); N], counts: [0; N], other: 0 }
    }

    pub fn observe(&mut self, item: T) {
        assert_eq!(self.buckets.len(), self.counts.len());
        for (bucket, count) in self.buckets.iter_mut().zip(self.counts.iter_mut()) {
            // TODO(max): Bubble up
            if *bucket == item {
                *count += 1;
                return;
            }
        }
        self.other += 1;
    }

    pub fn most_common(&self) -> Option<T> {
        // TODO(max): Return None if other count is >= sum of all other counts?
        self.buckets.iter().zip(self.counts.iter()).max_by(|l, r| l.1.cmp(&r.1)).map(|e| e.0).copied()
    }
}

#[cfg(test)]
mod distribution_tests {
    use super::*;

    #[test]
    fn start_empty() {
        let dist = Distribution::<usize, 4>::new();
        assert!(dist.buckets.is_empty());
        assert!(dist.counts.is_empty());
        assert_eq!(dist.other, 0);
    }

    #[test]
    fn observe_adds_record() {
        let mut dist = Distribution::<usize, 4>::new();
        dist.observe(10);
        assert_eq!(dist.buckets.len(), 1);
        assert_eq!(dist.counts.len(), 1);
        assert_eq!(dist.buckets[0], 10);
        assert_eq!(dist.counts[0], 1);
        assert_eq!(dist.other, 0);
    }

    #[test]
    fn observe_increments_record() {
        let mut dist = Distribution::<usize, 4>::new();
        dist.observe(10);
        dist.observe(10);
        assert_eq!(dist.buckets.len(), 1);
        assert_eq!(dist.counts.len(), 1);
        assert_eq!(dist.buckets[0], 10);
        assert_eq!(dist.counts[0], 2);
        assert_eq!(dist.other, 0);
    }

    #[test]
    fn observe_two() {
        let mut dist = Distribution::<usize, 4>::new();
        dist.observe(10);
        dist.observe(10);
        dist.observe(11);
        dist.observe(11);
        dist.observe(11);
        assert_eq!(dist.buckets.len(), 2);
        assert_eq!(dist.counts.len(), 2);
        assert_eq!(dist.buckets[0], 10);
        assert_eq!(dist.counts[0], 2);
        assert_eq!(dist.buckets[1], 11);
        assert_eq!(dist.counts[1], 3);
        assert_eq!(dist.other, 0);
    }

    #[test]
    fn observe_with_max_increments_other() {
        let mut dist = Distribution::<usize, 0>::new();
        dist.observe(10);
        assert!(dist.buckets.is_empty());
        assert!(dist.counts.is_empty());
        assert_eq!(dist.other, 1);
    }

    #[test]
    fn most_common_no_entries() {
        let dist = Distribution::<usize, 4>::new();
        assert_eq!(dist.most_common(), None);
    }

    #[test]
    fn most_common_only_other() {
        let mut dist = Distribution::<usize, 0>::new();
        dist.observe(10);
        assert_eq!(dist.most_common(), None);
    }

    #[test]
    fn most_common() {
        let mut dist = Distribution::<usize, 4>::new();
        dist.observe(10);
        dist.observe(10);
        dist.observe(11);
        dist.observe(11);
        dist.observe(11);
        dist.observe(12);
        dist.observe(12);
        assert_eq!(dist.most_common(), Some(11));
    }
}
