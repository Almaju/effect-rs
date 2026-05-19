//! [`HashMap<K, V>`] — a persistent HAMT map.

use std::fmt;
use std::hash::Hash;
use std::iter::FromIterator;

pub struct HashMap<K, V>(im::HashMap<K, V>);

impl<K, V> Clone for HashMap<K, V>
where
    K: Clone + Hash + Eq,
    V: Clone,
{
    fn clone(&self) -> Self {
        HashMap(self.0.clone())
    }
}

impl<K, V> fmt::Debug for HashMap<K, V>
where
    K: Clone + Hash + Eq + fmt::Debug,
    V: Clone + fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<K, V> PartialEq for HashMap<K, V>
where
    K: Clone + Hash + Eq,
    V: Clone + PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl<K, V> Eq for HashMap<K, V>
where
    K: Clone + Hash + Eq,
    V: Clone + Eq,
{
}

impl<K, V> Default for HashMap<K, V>
where
    K: Clone + Hash + Eq,
    V: Clone,
{
    fn default() -> Self {
        Self::empty()
    }
}

impl<K, V> HashMap<K, V>
where
    K: Clone + Hash + Eq,
    V: Clone,
{
    pub fn empty() -> Self {
        HashMap(im::HashMap::new())
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.0.contains_key(key)
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        self.0.get(key)
    }

    /// Return a new map with `(key, value)` inserted (replacing any
    /// existing entry).
    pub fn insert(self, key: K, value: V) -> Self {
        let mut m = self.0;
        m.insert(key, value);
        HashMap(m)
    }

    /// Return a new map without the given key.
    pub fn remove(self, key: &K) -> Self {
        let mut m = self.0;
        m.remove(key);
        HashMap(m)
    }

    /// Right-biased union: entries from `other` overwrite `self`.
    pub fn union(self, other: Self) -> Self {
        HashMap(self.0.union(other.0))
    }

    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.0.iter()
    }

    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.0.keys()
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.0.values()
    }

    /// Transform every value with `f`.
    pub fn map_values<W: Clone>(self, mut f: impl FnMut(V) -> W) -> HashMap<K, W> {
        HashMap(self.0.into_iter().map(|(k, v)| (k, f(v))).collect())
    }
}

impl<K, V> FromIterator<(K, V)> for HashMap<K, V>
where
    K: Clone + Hash + Eq,
    V: Clone,
{
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        HashMap(iter.into_iter().collect())
    }
}

impl<K, V> IntoIterator for HashMap<K, V>
where
    K: Clone + Hash + Eq,
    V: Clone,
{
    type Item = (K, V);
    type IntoIter = im::hashmap::ConsumingIter<(K, V)>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_map_has_no_entries() {
        let m: HashMap<&str, i32> = HashMap::empty();
        assert!(m.is_empty());
        assert_eq!(m.len(), 0);
    }

    #[test]
    fn insert_and_get_roundtrip() {
        let m: HashMap<&str, i32> = HashMap::empty().insert("a", 1).insert("b", 2);
        assert_eq!(m.get(&"a"), Some(&1));
        assert_eq!(m.get(&"b"), Some(&2));
        assert_eq!(m.get(&"missing"), None);
    }

    #[test]
    fn insert_overwrites_existing_key() {
        let m: HashMap<&str, i32> = HashMap::empty().insert("a", 1).insert("a", 99);
        assert_eq!(m.get(&"a"), Some(&99));
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn remove_drops_entry() {
        let m: HashMap<&str, i32> = HashMap::empty().insert("a", 1).insert("b", 2).remove(&"a");
        assert_eq!(m.contains_key(&"a"), false);
        assert_eq!(m.get(&"b"), Some(&2));
    }

    #[test]
    fn union_is_right_biased() {
        let a: HashMap<&str, i32> = HashMap::empty().insert("a", 1);
        let b: HashMap<&str, i32> = HashMap::empty().insert("a", 99).insert("b", 2);
        let u = a.union(b);
        assert_eq!(u.get(&"a"), Some(&99));
        assert_eq!(u.get(&"b"), Some(&2));
    }

    #[test]
    fn map_values_transforms() {
        let m: HashMap<&str, i32> = HashMap::empty().insert("a", 1).insert("b", 2);
        let doubled = m.map_values(|v| v * 2);
        assert_eq!(doubled.get(&"a"), Some(&2));
        assert_eq!(doubled.get(&"b"), Some(&4));
    }

    #[test]
    fn from_iter_collects() {
        let m: HashMap<&str, i32> = [("a", 1), ("b", 2)].into_iter().collect();
        assert_eq!(m.len(), 2);
    }
}
