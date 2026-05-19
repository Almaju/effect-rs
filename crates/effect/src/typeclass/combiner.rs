//! [`Combiner`] — a binary associative combination, optionally with an
//! identity element. Effect-TS calls this `Semigroup`/`Monoid`.

/// A binary combining operation. `combine` must be associative:
/// `combine(combine(a, b), c) == combine(a, combine(b, c))`.
pub trait Combiner<A> {
    fn combine(&self, a: A, b: A) -> A;

    /// Identity element, if one exists. When `Some`, this combiner is
    /// a monoid; when `None`, it's only a semigroup.
    fn empty(&self) -> Option<A> {
        None
    }

    /// Fold an iterable using this combiner. Returns the identity for
    /// an empty iterator if available, otherwise `None`.
    fn combine_all<I: IntoIterator<Item = A>>(&self, items: I) -> Option<A> {
        let mut iter = items.into_iter();
        match (iter.next(), self.empty()) {
            (None, e) => e,
            (Some(first), _) => Some(iter.fold(first, |acc, x| self.combine(acc, x))),
        }
    }
}

/// String concatenation. Identity is the empty string.
pub struct Concat;

impl Combiner<String> for Concat {
    fn combine(&self, mut a: String, b: String) -> String {
        a.push_str(&b);
        a
    }
    fn empty(&self) -> Option<String> {
        Some(String::new())
    }
}

/// Integer addition. Identity is zero.
pub struct Sum;

macro_rules! impl_sum {
    ($($t:ty),*) => {
        $(
            impl Combiner<$t> for Sum {
                fn combine(&self, a: $t, b: $t) -> $t { a + b }
                fn empty(&self) -> Option<$t> { Some(0) }
            }
        )*
    };
}
impl_sum!(i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize);

/// Integer/float multiplication. Identity is one.
pub struct Product;

macro_rules! impl_product {
    ($($t:ty),*) => {
        $(
            impl Combiner<$t> for Product {
                fn combine(&self, a: $t, b: $t) -> $t { a * b }
                fn empty(&self) -> Option<$t> { Some(1) }
            }
        )*
    };
}
impl_product!(i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize);

/// Vec concatenation. Identity is `Vec::new()`.
pub struct VecConcat;

impl<T> Combiner<Vec<T>> for VecConcat {
    fn combine(&self, mut a: Vec<T>, mut b: Vec<T>) -> Vec<T> {
        a.append(&mut b);
        a
    }
    fn empty(&self) -> Option<Vec<T>> {
        Some(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sum_adds() {
        assert_eq!(Sum.combine(2, 3), 5);
        assert_eq!(Sum.empty(), Some(0_i32));
    }

    #[test]
    fn product_multiplies() {
        assert_eq!(Product.combine(2, 3), 6);
        assert_eq!(Product.empty(), Some(1_i32));
    }

    #[test]
    fn concat_concatenates() {
        let combined = Concat.combine("foo".to_string(), "bar".to_string());
        assert_eq!(combined, "foobar");
    }

    #[test]
    fn combine_all_folds_iterable() {
        assert_eq!(Sum.combine_all(vec![1, 2, 3, 4]), Some(10));
        let empty: Vec<i32> = vec![];
        assert_eq!(Sum.combine_all(empty), Some(0));   // identity present
    }

    #[test]
    fn vec_concat_combines() {
        let combined = VecConcat.combine(vec![1, 2], vec![3, 4]);
        assert_eq!(combined, vec![1, 2, 3, 4]);
    }
}
