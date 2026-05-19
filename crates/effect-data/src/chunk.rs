//! [`Chunk<A>`] — an immutable sequence with O(log n) push, pop, and
//! random access. Backed by [`im::Vector`].

use std::fmt;
use std::iter::FromIterator;

/// An immutable sequence.
pub struct Chunk<A>(im::Vector<A>);

impl<A: Clone> Clone for Chunk<A> {
    fn clone(&self) -> Self {
        Chunk(self.0.clone())
    }
}

impl<A: Clone + fmt::Debug> fmt::Debug for Chunk<A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<A: Clone + PartialEq> PartialEq for Chunk<A> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl<A: Clone + Eq> Eq for Chunk<A> {}

impl<A> Default for Chunk<A>
where
    A: Clone,
{
    fn default() -> Self {
        Self::empty()
    }
}

impl<A> Chunk<A>
where
    A: Clone,
{
    /// An empty chunk.
    pub fn empty() -> Self {
        Chunk(im::Vector::new())
    }

    /// A chunk containing a single element.
    pub fn singleton(value: A) -> Self {
        let mut v = im::Vector::new();
        v.push_back(value);
        Chunk(v)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&self, idx: usize) -> Option<&A> {
        self.0.get(idx)
    }

    pub fn first(&self) -> Option<&A> {
        self.0.front()
    }

    pub fn last(&self) -> Option<&A> {
        self.0.back()
    }

    /// Return a new chunk with `value` appended.
    pub fn push_back(self, value: A) -> Self {
        let mut v = self.0;
        v.push_back(value);
        Chunk(v)
    }

    /// Return a new chunk with `value` prepended.
    pub fn push_front(self, value: A) -> Self {
        let mut v = self.0;
        v.push_front(value);
        Chunk(v)
    }

    /// Concatenate two chunks. O(log min(m, n)).
    pub fn concat(self, other: Self) -> Self {
        let mut a = self.0;
        a.append(other.0);
        Chunk(a)
    }

    /// Apply `f` to every element, producing a new chunk.
    pub fn map<B: Clone>(self, f: impl FnMut(A) -> B) -> Chunk<B> {
        Chunk(self.0.into_iter().map(f).collect())
    }

    /// Keep only elements satisfying `f`.
    pub fn filter(self, mut f: impl FnMut(&A) -> bool) -> Self {
        Chunk(self.0.into_iter().filter(|a| f(a)).collect())
    }

    /// Left fold.
    pub fn fold<B>(self, init: B, f: impl FnMut(B, A) -> B) -> B {
        self.0.into_iter().fold(init, f)
    }

    /// Iterate by reference.
    pub fn iter(&self) -> im::vector::Iter<'_, A> {
        self.0.iter()
    }

    /// Convert into a `Vec`. O(n).
    pub fn into_vec(self) -> Vec<A> {
        self.0.into_iter().collect()
    }
}

impl<A: Clone> FromIterator<A> for Chunk<A> {
    fn from_iter<I: IntoIterator<Item = A>>(iter: I) -> Self {
        Chunk(iter.into_iter().collect())
    }
}

impl<A: Clone> IntoIterator for Chunk<A> {
    type Item = A;
    type IntoIter = im::vector::ConsumingIter<A>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a, A: Clone> IntoIterator for &'a Chunk<A> {
    type Item = &'a A;
    type IntoIter = im::vector::Iter<'a, A>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_chunk_has_no_elements() {
        let c: Chunk<i32> = Chunk::empty();
        assert_eq!(c.len(), 0);
        assert!(c.is_empty());
    }

    #[test]
    fn singleton_has_one_element() {
        let c = Chunk::singleton(42);
        assert_eq!(c.len(), 1);
        assert_eq!(c.get(0), Some(&42));
    }

    #[test]
    fn push_back_appends() {
        let c = Chunk::empty().push_back(1).push_back(2).push_back(3);
        assert_eq!(c.into_vec(), vec![1, 2, 3]);
    }

    #[test]
    fn push_front_prepends() {
        let c = Chunk::empty().push_front(1).push_front(2).push_front(3);
        assert_eq!(c.into_vec(), vec![3, 2, 1]);
    }

    #[test]
    fn concat_joins_two_chunks() {
        let a = Chunk::from_iter([1, 2]);
        let b = Chunk::from_iter([3, 4]);
        assert_eq!(a.concat(b).into_vec(), vec![1, 2, 3, 4]);
    }

    #[test]
    fn map_transforms() {
        let c = Chunk::from_iter([1, 2, 3]).map(|x| x * 2);
        assert_eq!(c.into_vec(), vec![2, 4, 6]);
    }

    #[test]
    fn filter_keeps_matching() {
        let c = Chunk::from_iter([1, 2, 3, 4]).filter(|x| x % 2 == 0);
        assert_eq!(c.into_vec(), vec![2, 4]);
    }

    #[test]
    fn fold_left_reduces() {
        let sum = Chunk::from_iter([1, 2, 3, 4]).fold(0, |acc, x| acc + x);
        assert_eq!(sum, 10);
    }

    #[test]
    fn first_last_return_endpoints() {
        let c = Chunk::from_iter([1, 2, 3]);
        assert_eq!(c.first(), Some(&1));
        assert_eq!(c.last(), Some(&3));
    }

    #[test]
    fn clone_shares_structure() {
        // Just verify Clone is cheap and produces an equal chunk.
        let a = Chunk::from_iter(0..100);
        let b = a.clone();
        assert_eq!(a, b);
    }
}
