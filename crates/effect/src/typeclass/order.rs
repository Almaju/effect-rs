//! [`Order`] — a value-level ordering.
//!
//! Like [`Equivalence`](super::Equivalence), this is useful when a
//! type can be ordered multiple ways (ascending, descending,
//! by-some-field) and `Ord` would have to pick one canonical answer.

use std::cmp::Ordering;
use std::marker::PhantomData;

pub trait Order<A: ?Sized> {
    fn compare(&self, a: &A, b: &A) -> Ordering;
}

/// The natural order delegating to `Ord`.
pub struct NaturalOrder<A: ?Sized>(PhantomData<fn() -> A>);

impl<A: ?Sized> NaturalOrder<A> {
    pub const fn new() -> Self {
        NaturalOrder(PhantomData)
    }
}

impl<A: ?Sized + Ord> Order<A> for NaturalOrder<A> {
    fn compare(&self, a: &A, b: &A) -> Ordering {
        a.cmp(b)
    }
}

/// Reverse of any other order.
pub struct Reverse<O>(pub O);

impl<A: ?Sized, O: Order<A>> Order<A> for Reverse<O> {
    fn compare(&self, a: &A, b: &A) -> Ordering {
        self.0.compare(a, b).reverse()
    }
}

/// Order by a derived key.
pub struct OrderBy<F> {
    pub key: F,
}

impl<F> OrderBy<F> {
    pub fn new(key: F) -> Self {
        OrderBy { key }
    }
}

impl<A: ?Sized, K: Ord, F: Fn(&A) -> K> Order<A> for OrderBy<F> {
    fn compare(&self, a: &A, b: &A) -> Ordering {
        (self.key)(a).cmp(&(self.key)(b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order_uses_ord() {
        let o = NaturalOrder::<i32>::new();
        assert_eq!(o.compare(&1, &2), Ordering::Less);
        assert_eq!(o.compare(&2, &2), Ordering::Equal);
        assert_eq!(o.compare(&3, &2), Ordering::Greater);
    }

    #[test]
    fn reverse_flips_order() {
        let rev = Reverse(NaturalOrder::<i32>::new());
        assert_eq!(rev.compare(&1, &2), Ordering::Greater);
    }

    #[test]
    fn order_by_uses_key() {
        let by_len = OrderBy::new(|s: &&str| s.len());
        assert_eq!(by_len.compare(&"a", &"bb"), Ordering::Less);
        assert_eq!(by_len.compare(&"bb", &"a"), Ordering::Greater);
    }
}
