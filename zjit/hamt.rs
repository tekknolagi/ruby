//! A persistent hash array mapped trie (HAMT).
//!
//! This is a Rust port of V8's `src/maglev/hamt.h`. Insertions and merges use
//! path copying, so unchanged subtrees are shared through [`Rc`]. Branches use
//! five hash bits per level and a bitmap-compressed child array. Distinct keys
//! with identical hashes are kept in a collision list.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

const BITS: u32 = 5;
const WIDTH: u32 = 1 << BITS;
const MASK: u64 = (WIDTH - 1) as u64;
const MAX_SHIFT: u32 = u64::BITS - BITS;

type NodeRef<K, V> = Rc<Node<K, V>>;

enum Node<K, V> {
    Leaf {
        key: K,
        value: V,
        hash: u64,
        next: Option<NodeRef<K, V>>,
    },
    Branch {
        bitmap: u32,
        children: Box<[NodeRef<K, V>]>,
    },
}

impl<K, V> Node<K, V> {
    fn leaf(&self) -> (&K, &V, u64, Option<&NodeRef<K, V>>) {
        match self {
            Self::Leaf {
                key,
                value,
                hash,
                next,
            } => (key, value, *hash, next.as_ref()),
            Self::Branch { .. } => unreachable!("collision list contains a branch"),
        }
    }
}

/// A persistent (immutable) hash array mapped trie.
///
/// Cloning a `Hamt` and inserting into it leaves the old map unchanged. An
/// insertion takes O(log32 N) expected time and allocates only the path from
/// the changed leaf to the root. `merge_into` is a left intersection update:
/// keys found only in `other` are not added.
pub struct Hamt<K, V> {
    root: Option<NodeRef<K, V>>,
}

impl<K, V> Clone for Hamt<K, V> {
    fn clone(&self) -> Self {
        Self {
            root: self.root.clone(),
        }
    }
}

impl<K, V> Default for Hamt<K, V> {
    fn default() -> Self {
        Self { root: None }
    }
}

impl<K, V> Hamt<K, V>
where
    K: Clone + Eq + Hash,
    V: Clone + PartialEq,
{
    /// Creates an empty HAMT.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the value associated with `key`.
    pub fn find(&self, key: &K) -> Option<&V> {
        let hash = hash_key(key);
        Self::find_helper(self.root.as_deref(), key, hash, 0)
    }

    /// Returns a new HAMT containing `key` mapped to `value`.
    ///
    /// If the map already contains the same key and value, this is a no-op and
    /// the returned HAMT shares its root with `self`.
    pub fn insert(&self, key: K, value: V) -> Self {
        let hash = hash_key(&key);
        Self {
            root: Some(Self::insert_rec(self.root.as_ref(), &key, &value, hash, 0)),
        }
    }

    /// Updates values in `self` with matching values from `other`.
    ///
    /// This operation is intentionally asymmetric. The callback receives
    /// `(self_value, other_value)`. Keys present only in `other` are ignored.
    /// Unchanged paths retain their original allocation.
    pub fn merge_into<F>(&self, other: &Self, merge: F) -> Self
    where
        F: Fn(&V, &V) -> V,
    {
        let Some(left) = self.root.as_ref() else {
            return self.clone();
        };
        let Some(right) = other.root.as_ref() else {
            return self.clone();
        };
        if Rc::ptr_eq(left, right) {
            return self.clone();
        }
        Self {
            root: Some(Self::merge_rec(left, right, 0, &merge)),
        }
    }

    /// Iterates over all key/value pairs in hash-trie order.
    pub fn iter(&self) -> Iter<'_, K, V> {
        Iter::new(self.root.as_deref())
    }

    fn new_leaf(key: K, value: V, hash: u64, next: Option<NodeRef<K, V>>) -> NodeRef<K, V> {
        Rc::new(Node::Leaf {
            key,
            value,
            hash,
            next,
        })
    }

    fn insert_collision(
        list: Option<&NodeRef<K, V>>,
        key: &K,
        value: &V,
        hash: u64,
    ) -> NodeRef<K, V> {
        let Some(node) = list else {
            return Self::new_leaf(key.clone(), value.clone(), hash, None);
        };
        let (old_key, old_value, old_hash, next) = node.leaf();
        if old_key == key {
            if old_value == value {
                return node.clone();
            }
            return Self::new_leaf(key.clone(), value.clone(), hash, next.cloned());
        }

        let new_next = Self::insert_collision(next, key, value, hash);
        if next.is_some_and(|old_next| Rc::ptr_eq(old_next, &new_next)) {
            return node.clone();
        }
        Self::new_leaf(old_key.clone(), old_value.clone(), old_hash, Some(new_next))
    }

    fn insert_leaf(
        node: &NodeRef<K, V>,
        key: &K,
        value: &V,
        hash: u64,
        shift: u32,
    ) -> NodeRef<K, V> {
        let (old_key, old_value, old_hash, next) = node.leaf();
        if old_key == key {
            if old_value == value {
                return node.clone();
            }
            return Self::new_leaf(key.clone(), value.clone(), hash, next.cloned());
        }

        if old_hash == hash || shift > MAX_SHIFT {
            return Self::insert_collision(Some(node), key, value, hash);
        }

        let old_bit = ((old_hash >> shift) & MASK) as u32;
        let new_bit = ((hash >> shift) & MASK) as u32;
        if old_bit != new_bit {
            let new_leaf = Self::new_leaf(key.clone(), value.clone(), hash, None);
            let children = if old_bit < new_bit {
                vec![node.clone(), new_leaf]
            } else {
                vec![new_leaf, node.clone()]
            };
            return Rc::new(Node::Branch {
                bitmap: (1 << old_bit) | (1 << new_bit),
                children: children.into_boxed_slice(),
            });
        }

        let child = Self::insert_rec(Some(node), key, value, hash, shift + BITS);
        Rc::new(Node::Branch {
            bitmap: 1 << old_bit,
            children: vec![child].into_boxed_slice(),
        })
    }

    fn insert_rec(
        node: Option<&NodeRef<K, V>>,
        key: &K,
        value: &V,
        hash: u64,
        shift: u32,
    ) -> NodeRef<K, V> {
        let Some(node) = node else {
            return Self::new_leaf(key.clone(), value.clone(), hash, None);
        };
        match node.as_ref() {
            Node::Leaf { .. } => Self::insert_leaf(node, key, value, hash, shift),
            Node::Branch { bitmap, children } => {
                let bit = ((hash >> shift) & MASK) as u32;
                let bit_mask = 1_u32 << bit;
                let index = index_for(*bitmap, bit);
                if bitmap & bit_mask != 0 {
                    let old_child = &children[index];
                    let new_child =
                        Self::insert_rec(Some(old_child), key, value, hash, shift + BITS);
                    if Rc::ptr_eq(old_child, &new_child) {
                        return node.clone();
                    }
                    let mut new_children = children.to_vec();
                    new_children[index] = new_child;
                    Rc::new(Node::Branch {
                        bitmap: *bitmap,
                        children: new_children.into_boxed_slice(),
                    })
                } else {
                    let mut new_children = Vec::with_capacity(children.len() + 1);
                    new_children.extend_from_slice(&children[..index]);
                    new_children.push(Self::new_leaf(key.clone(), value.clone(), hash, None));
                    new_children.extend_from_slice(&children[index..]);
                    Rc::new(Node::Branch {
                        bitmap: bitmap | bit_mask,
                        children: new_children.into_boxed_slice(),
                    })
                }
            }
        }
    }

    fn find_helper<'a>(
        mut node: Option<&'a Node<K, V>>,
        key: &K,
        hash: u64,
        mut shift: u32,
    ) -> Option<&'a V> {
        while let Some(current) = node {
            match current {
                Node::Leaf { .. } => return Self::find_collision(current, key),
                Node::Branch { bitmap, children } => {
                    let bit = ((hash >> shift) & MASK) as u32;
                    if bitmap & (1 << bit) == 0 {
                        return None;
                    }
                    node = Some(children[index_for(*bitmap, bit)].as_ref());
                    shift += BITS;
                }
            }
        }
        None
    }

    fn find_collision<'a>(mut node: &'a Node<K, V>, key: &K) -> Option<&'a V> {
        loop {
            let (old_key, value, _, next) = node.leaf();
            if old_key == key {
                return Some(value);
            }
            node = next?.as_ref();
        }
    }

    fn merge_collision<F>(left: &NodeRef<K, V>, right: &NodeRef<K, V>, merge: &F) -> NodeRef<K, V>
    where
        F: Fn(&V, &V) -> V,
    {
        let (key, old_value, hash, next) = left.leaf();
        let new_next = next.map(|next| Self::merge_collision(next, right, merge));
        let value = Self::find_collision(right.as_ref(), key)
            .map_or_else(|| old_value.clone(), |other| merge(old_value, other));
        let next_unchanged = match (next, new_next.as_ref()) {
            (None, None) => true,
            (Some(old), Some(new)) => Rc::ptr_eq(old, new),
            _ => false,
        };
        if next_unchanged && value == *old_value {
            return left.clone();
        }
        Self::new_leaf(key.clone(), value, hash, new_next)
    }

    fn merge_branch_leaf<F>(
        branch_node: &NodeRef<K, V>,
        leaf: &NodeRef<K, V>,
        shift: u32,
        merge: &F,
    ) -> NodeRef<K, V>
    where
        F: Fn(&V, &V) -> V,
    {
        let Node::Branch { bitmap, children } = branch_node.as_ref() else {
            unreachable!()
        };
        let (_, _, hash, _) = leaf.leaf();
        let bit = ((hash >> shift) & MASK) as u32;
        if bitmap & (1 << bit) == 0 {
            return branch_node.clone();
        }
        let index = index_for(*bitmap, bit);
        let merged = Self::merge_rec(&children[index], leaf, shift + BITS, merge);
        if Rc::ptr_eq(&children[index], &merged) {
            return branch_node.clone();
        }
        let mut new_children = children.to_vec();
        new_children[index] = merged;
        Rc::new(Node::Branch {
            bitmap: *bitmap,
            children: new_children.into_boxed_slice(),
        })
    }

    fn merge_leaf_branch<F>(
        leaf: &NodeRef<K, V>,
        branch: &NodeRef<K, V>,
        shift: u32,
        merge: &F,
    ) -> NodeRef<K, V>
    where
        F: Fn(&V, &V) -> V,
    {
        let (key, old_value, hash, next) = leaf.leaf();
        let new_next = next.map(|next| Self::merge_leaf_branch(next, branch, shift, merge));
        let value = Self::find_helper(Some(branch.as_ref()), key, hash, shift)
            .map_or_else(|| old_value.clone(), |other| merge(old_value, other));
        let next_unchanged = match (next, new_next.as_ref()) {
            (None, None) => true,
            (Some(old), Some(new)) => Rc::ptr_eq(old, new),
            _ => false,
        };
        if next_unchanged && value == *old_value {
            return leaf.clone();
        }
        Self::new_leaf(key.clone(), value, hash, new_next)
    }

    fn merge_rec<F>(
        left: &NodeRef<K, V>,
        right: &NodeRef<K, V>,
        shift: u32,
        merge: &F,
    ) -> NodeRef<K, V>
    where
        F: Fn(&V, &V) -> V,
    {
        if Rc::ptr_eq(left, right) {
            return left.clone();
        }
        match (left.as_ref(), right.as_ref()) {
            (Node::Leaf { .. }, Node::Leaf { .. }) => Self::merge_collision(left, right, merge),
            (Node::Leaf { .. }, Node::Branch { .. }) => {
                Self::merge_leaf_branch(left, right, shift, merge)
            }
            (Node::Branch { .. }, Node::Leaf { .. }) => {
                Self::merge_branch_leaf(left, right, shift, merge)
            }
            (
                Node::Branch {
                    bitmap: left_bitmap,
                    children: left_children,
                },
                Node::Branch {
                    bitmap: right_bitmap,
                    children: right_children,
                },
            ) => {
                let mut result: Option<Vec<NodeRef<K, V>>> = None;
                let mut shared = left_bitmap & right_bitmap;
                while shared != 0 {
                    let bit = shared.trailing_zeros();
                    let left_index = index_for(*left_bitmap, bit);
                    let right_index = index_for(*right_bitmap, bit);
                    let old_child = &left_children[left_index];
                    let child = Self::merge_rec(
                        old_child,
                        &right_children[right_index],
                        shift + BITS,
                        merge,
                    );
                    if !Rc::ptr_eq(old_child, &child) {
                        result.get_or_insert_with(|| left_children.to_vec())[left_index] = child;
                    }
                    shared &= shared - 1;
                }
                match result {
                    None => left.clone(),
                    Some(children) => Rc::new(Node::Branch {
                        bitmap: *left_bitmap,
                        children: children.into_boxed_slice(),
                    }),
                }
            }
        }
    }
}

impl<'a, K, V> IntoIterator for &'a Hamt<K, V>
where
    K: Clone + Eq + Hash,
    V: Clone + PartialEq,
{
    type Item = (&'a K, &'a V);
    type IntoIter = Iter<'a, K, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// An iterator over a [`Hamt`].
pub struct Iter<'a, K, V> {
    current: Option<&'a Node<K, V>>,
    stack: Vec<(&'a [NodeRef<K, V>], usize)>,
}

impl<'a, K, V> Iter<'a, K, V> {
    fn new(root: Option<&'a Node<K, V>>) -> Self {
        let mut iter = Self {
            current: None,
            stack: Vec::new(),
        };
        iter.descend(root);
        iter
    }

    fn descend(&mut self, mut node: Option<&'a Node<K, V>>) {
        while let Some(current) = node {
            match current {
                Node::Leaf { .. } => {
                    self.current = Some(current);
                    return;
                }
                Node::Branch { children, .. } => {
                    debug_assert!(!children.is_empty());
                    self.stack.push((children, 0));
                    node = Some(children[0].as_ref());
                }
            }
        }
        self.current = None;
    }

    fn advance_tree(&mut self) {
        while let Some((children, index)) = self.stack.last_mut() {
            *index += 1;
            if *index < children.len() {
                let child = children[*index].as_ref();
                self.descend(Some(child));
                return;
            }
            self.stack.pop();
        }
        self.current = None;
    }
}

impl<'a, K, V> Iterator for Iter<'a, K, V> {
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.current?;
        let Node::Leaf {
            key, value, next, ..
        } = current
        else {
            unreachable!()
        };
        if let Some(next) = next {
            self.current = Some(next.as_ref());
        } else {
            self.advance_tree();
        }
        Some((key, value))
    }
}

fn hash_key<K: Hash + ?Sized>(key: &K) -> u64 {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish()
}

fn index_for(bitmap: u32, bit: u32) -> usize {
    (bitmap & ((1_u32 << bit) - 1)).count_ones() as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    type IntHamt = Hamt<i32, i32>;

    fn insert_with_hash(map: &IntHamt, key: i32, value: i32, hash: u64) -> IntHamt {
        IntHamt {
            root: Some(IntHamt::insert_rec(
                map.root.as_ref(),
                &key,
                &value,
                hash,
                0,
            )),
        }
    }

    fn find_with_hash(map: &IntHamt, key: i32, hash: u64) -> Option<&i32> {
        IntHamt::find_helper(map.root.as_deref(), &key, hash, 0)
    }

    fn same_root(left: &IntHamt, right: &IntHamt) -> bool {
        match (&left.root, &right.root) {
            (None, None) => true,
            (Some(left), Some(right)) => Rc::ptr_eq(left, right),
            _ => false,
        }
    }

    fn to_map(map: &IntHamt) -> BTreeMap<i32, i32> {
        map.iter().map(|(&key, &value)| (key, value)).collect()
    }

    #[test]
    fn empty() {
        let map = IntHamt::new();
        assert_eq!(map.find(&100), None);
        assert_eq!(map.iter().next(), None);
    }

    #[test]
    fn insert_find_overwrite_and_persistence() {
        let h0 = IntHamt::new();
        let h1 = h0.insert(1, 100);
        let h2 = h1.insert(2, 200);
        let h3 = h2.insert(1, 999);

        assert_eq!(h0.find(&1), None);
        assert_eq!(h1.find(&1), Some(&100));
        assert_eq!(h1.find(&2), None);
        assert_eq!(h2.find(&1), Some(&100));
        assert_eq!(h2.find(&2), Some(&200));
        assert_eq!(h3.find(&1), Some(&999));
        assert!(same_root(&h3, &h3.insert(1, 999)));
    }

    #[test]
    fn large_insert_and_iteration() {
        let mut map = IntHamt::new();
        let mut expected = BTreeMap::new();
        for key in 0..2_000 {
            map = map.insert(key, key * 2);
            expected.insert(key, key * 2);
        }
        for (&key, &value) in &expected {
            assert_eq!(map.find(&key), Some(&value));
        }
        assert_eq!(to_map(&map), expected);
    }

    #[test]
    fn exact_hash_collision_and_update() {
        let hash = 0xdead_beef;
        let h0 = insert_with_hash(&IntHamt::new(), 1, 10, hash);
        let h0 = insert_with_hash(&h0, 2, 20, hash);
        let h0 = insert_with_hash(&h0, 3, 30, hash);
        let h1 = insert_with_hash(&h0, 2, 222, hash);

        assert_eq!(find_with_hash(&h0, 1, hash), Some(&10));
        assert_eq!(find_with_hash(&h0, 2, hash), Some(&20));
        assert_eq!(find_with_hash(&h0, 3, hash), Some(&30));
        assert_eq!(find_with_hash(&h0, 4, hash), None);
        assert_eq!(find_with_hash(&h1, 1, hash), Some(&10));
        assert_eq!(find_with_hash(&h1, 2, hash), Some(&222));
        assert_eq!(find_with_hash(&h1, 3, hash), Some(&30));
    }

    #[test]
    fn deep_partial_hash_collision() {
        let h0 = IntHamt::new();
        let h1 = insert_with_hash(&h0, 1, 100, 0);
        let h2 = insert_with_hash(&h1, 2, 200, 1 << 10);

        assert_eq!(find_with_hash(&h0, 1, 0), None);
        assert_eq!(find_with_hash(&h1, 1, 0), Some(&100));
        assert_eq!(find_with_hash(&h1, 2, 1 << 10), None);
        assert_eq!(find_with_hash(&h2, 1, 0), Some(&100));
        assert_eq!(find_with_hash(&h2, 2, 1 << 10), Some(&200));
    }

    #[test]
    fn merge_is_left_join() {
        let left = IntHamt::new().insert(1, 10).insert(2, 20);
        let right = IntHamt::new().insert(2, 200).insert(3, 300);
        let result = left.merge_into(&right, |left, right| left + right);

        assert_eq!(result.find(&1), Some(&10));
        assert_eq!(result.find(&2), Some(&220));
        assert_eq!(result.find(&3), None);
        assert_eq!(left.find(&2), Some(&20));
    }

    #[test]
    fn unchanged_merge_reuses_root() {
        let mut left = IntHamt::new();
        let mut right = IntHamt::new();
        for key in 0..200 {
            left = left.insert(key, key);
            right = right.insert(key, key - 1);
        }
        let result = left.merge_into(&right, |left, right| *left.max(right));
        assert!(same_root(&left, &result));
    }

    #[test]
    fn merge_handles_collision_lists() {
        let hash = 0x1234;
        let mut left = IntHamt::new();
        let mut right = IntHamt::new();
        for key in 0..10 {
            left = insert_with_hash(&left, key, key, hash);
            right = insert_with_hash(&right, key, key - 1, hash);
        }
        let unchanged = left.merge_into(&right, |left, right| *left.max(right));
        assert!(same_root(&left, &unchanged));

        right = insert_with_hash(&right, 5, 500, hash);
        let changed = left.merge_into(&right, |left, right| *left.max(right));
        assert_eq!(find_with_hash(&changed, 5, hash), Some(&500));
        assert_eq!(find_with_hash(&left, 5, hash), Some(&5));
    }

    #[test]
    fn merge_handles_mismatched_shapes() {
        fn small(v1: i32, v2: i32) -> IntHamt {
            let map = insert_with_hash(&IntHamt::new(), 1, v1, 0x01);
            insert_with_hash(&map, 2, v2, 0x02)
        }
        fn big(v1: i32, v2: i32, v3: i32) -> IntHamt {
            let map = insert_with_hash(&IntHamt::new(), 1, v1, 0x01);
            let map = insert_with_hash(&map, 3, v3, 0x21);
            insert_with_hash(&map, 2, v2, 0x02)
        }

        let small = small(10, 20);
        let big = big(50, 5, 5);
        let result = small.merge_into(&big, |left, right| *left.max(right));
        assert_eq!(to_map(&result), BTreeMap::from([(1, 50), (2, 20)]));

        let result = big.merge_into(&small, |left, right| *left.min(right));
        assert_eq!(to_map(&result), BTreeMap::from([(1, 10), (2, 5), (3, 5)]));
    }
}
