#[derive(Debug, Clone)]
pub struct Distribution<T: Copy + PartialEq> {
    /// buckets and counts have the same length
    buckets: Vec<T>,
    counts: Vec<usize>,
    /// if there is no more room, increment the fallback
    other: usize,
    max_num_buckets: usize,
}

impl<T: Copy + PartialEq> Distribution<T> {
    pub fn new(max_num_buckets: usize) -> Self {
        Self { buckets: vec![], counts: vec![], other: 0, max_num_buckets }
    }

    pub fn observe(&mut self, item: T) {
        assert_eq!(self.buckets.len(), self.counts.len());
        for (bucket, count) in self.buckets.iter_mut().zip(self.counts.iter_mut()) {
            if *bucket == item {
                *count += 1;
                return;
            }
        }
        if self.buckets.len() < self.max_num_buckets {
            self.buckets.push(item);
            self.counts.push(1);
            return;
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
        let dist = Distribution::<usize>::new(4);
        assert!(dist.buckets.is_empty());
        assert!(dist.counts.is_empty());
        assert_eq!(dist.other, 0);
        assert_eq!(dist.max_num_buckets, 4);
    }

    #[test]
    fn observe_adds_record() {
        let mut dist = Distribution::<usize>::new(4);
        dist.observe(10);
        assert_eq!(dist.buckets.len(), 1);
        assert_eq!(dist.counts.len(), 1);
        assert_eq!(dist.buckets[0], 10);
        assert_eq!(dist.counts[0], 1);
        assert_eq!(dist.other, 0);
    }

    #[test]
    fn observe_increments_record() {
        let mut dist = Distribution::<usize>::new(4);
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
        let mut dist = Distribution::<usize>::new(4);
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
        let mut dist = Distribution::<usize>::new(0);
        dist.observe(10);
        assert!(dist.buckets.is_empty());
        assert!(dist.counts.is_empty());
        assert_eq!(dist.other, 1);
    }

    #[test]
    fn most_common_no_entries() {
        let dist = Distribution::<usize>::new(4);
        assert_eq!(dist.most_common(), None);
    }

    #[test]
    fn most_common_only_other() {
        let mut dist = Distribution::<usize>::new(0);
        dist.observe(10);
        assert_eq!(dist.most_common(), None);
    }

    #[test]
    fn most_common() {
        let mut dist = Distribution::<usize>::new(4);
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
