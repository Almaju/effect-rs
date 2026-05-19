//! [`Reducer`] — a left-fold over an iterable, parameterized by a step
//! function.
//!
//! This is a trait-style version of `Iterator::fold`. It's useful as a
//! generic interface when you want to abstract "reduce an iterable
//! down to a value" across multiple call sites.

pub trait Reducer<A, B> {
    fn step(&self, acc: B, item: A) -> B;

    /// Reduce an iterable starting from `initial`.
    fn reduce<I: IntoIterator<Item = A>>(&self, initial: B, items: I) -> B {
        items.into_iter().fold(initial, |acc, item| self.step(acc, item))
    }
}

/// Reduce using a closure.
pub struct FnReducer<F>(pub F);

impl<A, B, F: Fn(B, A) -> B> Reducer<A, B> for FnReducer<F> {
    fn step(&self, acc: B, item: A) -> B {
        (self.0)(acc, item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fn_reducer_sums() {
        let r = FnReducer(|acc: i32, x: i32| acc + x);
        assert_eq!(r.reduce(0, vec![1, 2, 3, 4]), 10);
    }

    #[test]
    fn fn_reducer_collects() {
        let r = FnReducer(|mut acc: Vec<i32>, x: i32| {
            acc.push(x * 2);
            acc
        });
        assert_eq!(r.reduce(Vec::new(), vec![1, 2, 3]), vec![2, 4, 6]);
    }
}
