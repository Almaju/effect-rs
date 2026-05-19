//! [`HashSet<A>`] — a persistent HAMT set.

use std::fmt;
use std::hash::Hash;
use std::iter::FromIterator;

pub struct HashSet<A>(im::HashSet<A>);

impl<A: Clone + Hash + Eq> Clone for HashSet<A> {
    fn clone(&self) -> Self {
        HashSet(self.0.clone())
    }
}

impl<A: Clone + Hash + Eq + fmt::Debug> fmt::Debug for HashSet<A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<A: Clone + Hash + Eq> PartialEq for HashSet<A> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl<A: Clone + Hash + Eq> Eq for HashSet<A> {}

impl<A> Default for HashSet<A>
where
    A: Clone + Hash + Eq,
{
    fn default() -> Self {
        Self::empty()
    }
}

impl<A> HashSet<A>
where
    A: Clone + Hash + Eq,
{
    pub fn empty() -> Self {
        HashSet(im::HashSet::new())
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn contains(&self, value: &A) -> bool {
        self.0.contains(value)
    }

    /// Return a new set with `value` inserted.
    pub fn insert(self, value: A) -> Self {
        let mut s = self.0;
        s.insert(value);
        HashSet(s)
    }

    /// Return a new set without `value`.
    pub fn remove(self, value: &A) -> Self {
        let mut s = self.0;
        s.remove(value);
        HashSet(s)
    }

    pub fn union(self, other: Self) -> Self {
        HashSet(self.0.union(other.0))
    }

    pub fn intersection(self, other: Self) -> Self {
        HashSet(self.0.intersection(other.0))
    }

    pub fn difference(self, other: Self) -> Self {
        HashSet(self.0.difference(other.0))
    }

    pub fn iter(&self) -> impl Iterator<Item = &A> {
        self.0.iter()
    }
}

impl<A> FromIterator<A> for HashSet<A>
where
    A: Clone + Hash + Eq,
{
    fn from_iter<I: IntoIterator<Item = A>>(iter: I) -> Self {
        HashSet(iter.into_iter().collect())
    }
}

impl<A> IntoIterator for HashSet<A>
where
    A: Clone + Hash + Eq,
{
    type Item = A;
    type IntoIter = im::hashset::ConsumingIter<A>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_set_has_no_elements() {
        let s: HashSet<i32> = HashSet::empty();
        assert!(s.is_empty());
    }

    #[test]
    fn insert_and_contains() {
        let s: HashSet<i32> = HashSet::empty().insert(1).insert(2);
        assert!(s.contains(&1));
        assert!(s.contains(&2));
        assert!(!s.contains(&3));
    }

    #[test]
    fn insert_is_idempotent() {
        let s: HashSet<i32> = HashSet::empty().insert(1).insert(1);
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn remove_drops_value() {
        let s: HashSet<i32> = HashSet::empty().insert(1).insert(2).remove(&1);
        assert!(!s.contains(&1));
        assert!(s.contains(&2));
    }

    #[test]
    fn union_combines_both() {
        let a: HashSet<i32> = [1, 2, 3].into_iter().collect();
        let b: HashSet<i32> = [3, 4, 5].into_iter().collect();
        let u = a.union(b);
        assert_eq!(u.len(), 5);
    }

    #[test]
    fn intersection_keeps_shared() {
        let a: HashSet<i32> = [1, 2, 3].into_iter().collect();
        let b: HashSet<i32> = [2, 3, 4].into_iter().collect();
        let i = a.intersection(b);
        assert!(i.contains(&2));
        assert!(i.contains(&3));
        assert!(!i.contains(&1));
        assert_eq!(i.len(), 2);
    }

    #[test]
    fn difference_subtracts() {
        let a: HashSet<i32> = [1, 2, 3].into_iter().collect();
        let b: HashSet<i32> = [2, 3].into_iter().collect();
        let d = a.difference(b);
        assert!(d.contains(&1));
        assert_eq!(d.len(), 1);
    }
}
