//! [`Queue<A>`] — a multi-producer / multi-consumer async queue.
//!
//! Backed by [`async-channel`], with `Effect`-typed `offer` / `take` /
//! `shutdown` operations. Both bounded (with back-pressure) and
//! unbounded constructors are provided.

use async_channel::{Receiver, Sender};

use crate::Effect;

/// A shareable async queue.
pub struct Queue<A> {
    sender: Sender<A>,
    receiver: Receiver<A>,
}

impl<A> Clone for Queue<A> {
    fn clone(&self) -> Self {
        Queue {
            sender: self.sender.clone(),
            receiver: self.receiver.clone(),
        }
    }
}

impl<A> Queue<A>
where
    A: Clone + Send + Sync + 'static,
{
    /// Construct an unbounded queue. `offer` never back-pressures.
    pub fn new_unbounded() -> Self {
        let (sender, receiver) = async_channel::unbounded();
        Queue { sender, receiver }
    }

    /// Construct a bounded queue. `offer` suspends until there is space.
    pub fn new_bounded(capacity: usize) -> Self {
        let (sender, receiver) = async_channel::bounded(capacity);
        Queue { sender, receiver }
    }

    /// Send a value. For bounded queues this suspends until there is
    /// space. Returns `false` if the queue has been shut down.
    pub fn offer<E, R>(&self, value: A) -> Effect<bool, E, R>
    where
        E: Send + 'static,
        R: Send + Sync + 'static,
    {
        let sender = self.sender.clone();
        Effect::from_fn(move |_| {
            let sender = sender.clone();
            let value = value.clone();
            Box::pin(async move { Ok(sender.send(value).await.is_ok()) })
        })
    }

    /// Try to send without suspending. Returns `false` if the queue is
    /// full or shut down.
    pub fn try_offer<E, R>(&self, value: A) -> Effect<bool, E, R>
    where
        E: Send + 'static,
        R: Send + Sync + 'static,
    {
        let sender = self.sender.clone();
        Effect::sync(move || Ok(sender.try_send(value.clone()).is_ok()))
    }

    /// Suspend until a value is available. Returns `None` when the queue
    /// is empty and has been shut down.
    pub fn take<E, R>(&self) -> Effect<Option<A>, E, R>
    where
        E: Send + 'static,
        R: Send + Sync + 'static,
    {
        let receiver = self.receiver.clone();
        Effect::from_fn(move |_| {
            let receiver = receiver.clone();
            Box::pin(async move { Ok(receiver.recv().await.ok()) })
        })
    }

    /// Non-blocking take. Returns `None` if the queue is empty (even if
    /// not shut down).
    pub fn try_take<E, R>(&self) -> Effect<Option<A>, E, R>
    where
        E: Send + 'static,
        R: Send + Sync + 'static,
    {
        let receiver = self.receiver.clone();
        Effect::sync(move || Ok(receiver.try_recv().ok()))
    }

    /// Shut the queue down. Subsequent `offer` calls return `false`;
    /// `take` drains the remaining buffered items, then returns `None`.
    pub fn shutdown<E, R>(&self) -> Effect<(), E, R>
    where
        E: Send + 'static,
        R: Send + Sync + 'static,
    {
        let sender = self.sender.clone();
        Effect::sync(move || {
            sender.close();
            Ok(())
        })
    }

    /// Current number of buffered items.
    pub fn size<E, R>(&self) -> Effect<usize, E, R>
    where
        E: Send + 'static,
        R: Send + Sync + 'static,
    {
        let receiver = self.receiver.clone();
        Effect::sync(move || Ok(receiver.len()))
    }

    /// Whether the queue is empty.
    pub fn is_empty<E, R>(&self) -> Effect<bool, E, R>
    where
        E: Send + 'static,
        R: Send + Sync + 'static,
    {
        let receiver = self.receiver.clone();
        Effect::sync(move || Ok(receiver.is_empty()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn offer_and_take_preserve_fifo_order() {
        let q: Queue<i32> = Queue::new_unbounded();
        for i in 0..5 {
            q.offer::<String, ()>(i).execute().await;
        }
        let mut taken = Vec::new();
        for _ in 0..5 {
            taken.push(q.take::<String, ()>().execute().await.ok().flatten().unwrap());
        }
        assert_eq!(taken, vec![0, 1, 2, 3, 4]);
    }

    #[tokio::test]
    async fn take_suspends_until_offer() {
        let q: Queue<i32> = Queue::new_unbounded();
        let q_writer = q.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            q_writer.offer::<String, ()>(42).execute().await;
        });
        let got = q.take::<String, ()>().execute().await;
        assert_eq!(got.ok().flatten(), Some(42));
    }

    #[tokio::test]
    async fn try_take_on_empty_returns_none() {
        let q: Queue<i32> = Queue::new_unbounded();
        let got = q.try_take::<String, ()>().execute().await;
        assert_eq!(got.ok().flatten(), None);
    }

    #[tokio::test]
    async fn try_offer_on_full_bounded_returns_false() {
        let q: Queue<i32> = Queue::new_bounded(2);
        assert_eq!(q.try_offer::<String, ()>(1).execute().await.ok(), Some(true));
        assert_eq!(q.try_offer::<String, ()>(2).execute().await.ok(), Some(true));
        assert_eq!(q.try_offer::<String, ()>(3).execute().await.ok(), Some(false));
    }

    #[tokio::test]
    async fn bounded_offer_back_pressures_until_drained() {
        let q: Queue<i32> = Queue::new_bounded(1);
        q.offer::<String, ()>(1).execute().await;

        let q_writer = q.clone();
        let producer = tokio::spawn(async move {
            // This will suspend until something is taken.
            q_writer.offer::<String, ()>(2).execute().await;
        });

        // Give the producer a moment to attempt and block.
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!producer.is_finished(), "producer should still be blocked");

        let first = q.take::<String, ()>().execute().await;
        assert_eq!(first.ok().flatten(), Some(1));

        producer.await.unwrap();
        let second = q.take::<String, ()>().execute().await;
        assert_eq!(second.ok().flatten(), Some(2));
    }

    #[tokio::test]
    async fn shutdown_drains_then_returns_none() {
        let q: Queue<i32> = Queue::new_unbounded();
        q.offer::<String, ()>(1).execute().await;
        q.offer::<String, ()>(2).execute().await;
        q.shutdown::<String, ()>().execute().await;
        // Buffered items drain
        assert_eq!(q.take::<String, ()>().execute().await.ok().flatten(), Some(1));
        assert_eq!(q.take::<String, ()>().execute().await.ok().flatten(), Some(2));
        // Then take returns None
        assert_eq!(q.take::<String, ()>().execute().await.ok().flatten(), None);
        // And offer is rejected
        assert_eq!(q.offer::<String, ()>(3).execute().await.ok(), Some(false));
    }

    #[tokio::test]
    async fn size_and_is_empty_track_state() {
        let q: Queue<i32> = Queue::new_unbounded();
        assert_eq!(q.size::<String, ()>().execute().await.ok(), Some(0));
        assert_eq!(q.is_empty::<String, ()>().execute().await.ok(), Some(true));
        q.offer::<String, ()>(1).execute().await;
        q.offer::<String, ()>(2).execute().await;
        assert_eq!(q.size::<String, ()>().execute().await.ok(), Some(2));
        assert_eq!(q.is_empty::<String, ()>().execute().await.ok(), Some(false));
    }
}
