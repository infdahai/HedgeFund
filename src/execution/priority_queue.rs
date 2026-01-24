use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::sync::Mutex;
use std::time::Instant;

use crate::types::{OrderIntent, OrderPriority};

pub struct PushResult {
    pub accepted: bool,
    pub dropped: Option<PrioritizedOrder>,
}

pub struct PrioritizedOrder {
    pub order: OrderIntent,
    pub priority: OrderPriority,
    pub enqueue_time: Instant,
    pub deadline: Option<Instant>,
    sequence: u64,
}

impl PrioritizedOrder {
    pub fn new(order: OrderIntent, priority: OrderPriority, deadline: Option<Instant>) -> Self {
        Self {
            order,
            priority,
            enqueue_time: Instant::now(),
            deadline,
            sequence: 0,
        }
    }

    pub fn is_expired(&self) -> bool {
        self.deadline.map(|d| Instant::now() > d).unwrap_or(false)
    }
}

impl Eq for PrioritizedOrder {}

impl PartialEq for PrioritizedOrder {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority && self.sequence == other.sequence
    }
}

impl Ord for PrioritizedOrder {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.priority as u8).cmp(&(other.priority as u8)) {
            Ordering::Equal => other.sequence.cmp(&self.sequence),
            ord => ord,
        }
    }
}

impl PartialOrd for PrioritizedOrder {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub struct PriorityQueue {
    heap: Mutex<BinaryHeap<PrioritizedOrder>>,
    sequence_counter: Mutex<u64>,
    max_size: usize,
}

impl PriorityQueue {
    pub fn new(max_size: usize) -> Self {
        Self {
            heap: Mutex::new(BinaryHeap::new()),
            sequence_counter: Mutex::new(0),
            max_size,
        }
    }

    pub fn push(&self, mut order: PrioritizedOrder) -> PushResult {
        let mut heap = self.heap.lock().unwrap();

        let mut seq = self.sequence_counter.lock().unwrap();
        order.sequence = *seq;
        *seq += 1;

        if heap.len() < self.max_size {
            heap.push(order);
            return PushResult {
                accepted: true,
                dropped: None,
            };
        }

        let new_sequence = order.sequence;
        let mut items: Vec<PrioritizedOrder> = heap.drain().collect();
        items.push(order);
        let drop_index = items
            .iter()
            .enumerate()
            .min_by(|(_, left), (_, right)| compare_for_drop(left, right))
            .map(|(index, _)| index)
            .unwrap_or(0);
        let dropped = items.swap_remove(drop_index);

        for item in items {
            heap.push(item);
        }

        PushResult {
            accepted: dropped.sequence != new_sequence,
            dropped: Some(dropped),
        }
    }

    pub fn pop(&self) -> Option<PrioritizedOrder> {
        let mut heap = self.heap.lock().unwrap();

        while let Some(order) = heap.pop() {
            if !order.is_expired() {
                return Some(order);
            }
        }

        None
    }

    pub fn peek_priority(&self) -> Option<OrderPriority> {
        let heap = self.heap.lock().unwrap();
        heap.peek().map(|o| o.priority)
    }

    pub fn len(&self) -> usize {
        self.heap.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.heap.lock().unwrap().is_empty()
    }

    pub fn clear(&self) {
        self.heap.lock().unwrap().clear();
    }
}

impl Default for PriorityQueue {
    fn default() -> Self {
        Self::new(1024)
    }
}

fn compare_for_drop(left: &PrioritizedOrder, right: &PrioritizedOrder) -> Ordering {
    match (left.priority as u8).cmp(&(right.priority as u8)) {
        Ordering::Equal => left.sequence.cmp(&right.sequence),
        ordering => ordering,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::order::OrderSide;
    use alloy_primitives::U256;
    use std::time::Duration;

    fn make_order(market_id: &str) -> OrderIntent {
        OrderIntent::new(
            market_id,
            U256::from(1),
            OrderSide::Buy,
            0.5,
            U256::from(100),
        )
    }

    #[test]
    fn test_priority_ordering() {
        let queue = PriorityQueue::new(100);

        let _ = queue.push(PrioritizedOrder::new(
            make_order("low"),
            OrderPriority::Low,
            None,
        ));
        let _ = queue.push(PrioritizedOrder::new(
            make_order("high"),
            OrderPriority::High,
            None,
        ));
        let _ = queue.push(PrioritizedOrder::new(
            make_order("critical"),
            OrderPriority::Critical,
            None,
        ));
        let _ = queue.push(PrioritizedOrder::new(
            make_order("medium"),
            OrderPriority::Medium,
            None,
        ));

        assert_eq!(queue.pop().unwrap().order.market_id, "critical");
        assert_eq!(queue.pop().unwrap().order.market_id, "high");
        assert_eq!(queue.pop().unwrap().order.market_id, "medium");
        assert_eq!(queue.pop().unwrap().order.market_id, "low");
    }

    #[test]
    fn test_fifo_within_same_priority() {
        let queue = PriorityQueue::new(100);

        let _ = queue.push(PrioritizedOrder::new(
            make_order("first"),
            OrderPriority::Medium,
            None,
        ));
        let _ = queue.push(PrioritizedOrder::new(
            make_order("second"),
            OrderPriority::Medium,
            None,
        ));
        let _ = queue.push(PrioritizedOrder::new(
            make_order("third"),
            OrderPriority::Medium,
            None,
        ));

        assert_eq!(queue.pop().unwrap().order.market_id, "first");
        assert_eq!(queue.pop().unwrap().order.market_id, "second");
        assert_eq!(queue.pop().unwrap().order.market_id, "third");
    }

    #[test]
    fn test_expired_orders_skipped() {
        let queue = PriorityQueue::new(100);

        let past = Instant::now() - Duration::from_secs(1);
        let _ = queue.push(PrioritizedOrder::new(
            make_order("expired"),
            OrderPriority::High,
            Some(past),
        ));
        let _ = queue.push(PrioritizedOrder::new(
            make_order("valid"),
            OrderPriority::Low,
            None,
        ));

        assert_eq!(queue.pop().unwrap().order.market_id, "valid");
        assert!(queue.pop().is_none());
    }

    #[test]
    fn test_max_size_enforcement() {
        let queue = PriorityQueue::new(2);

        let first = queue.push(PrioritizedOrder::new(
            make_order("1"),
            OrderPriority::Low,
            None,
        ));
        assert!(first.accepted);
        let second = queue.push(PrioritizedOrder::new(
            make_order("2"),
            OrderPriority::Low,
            None,
        ));
        assert!(second.accepted);
        let third = queue.push(PrioritizedOrder::new(
            make_order("3"),
            OrderPriority::Low,
            None,
        ));
        assert!(third.dropped.is_some());

        assert_eq!(queue.len(), 2);
    }

    #[test]
    fn test_empty_queue() {
        let queue = PriorityQueue::new(100);
        assert!(queue.is_empty());
        assert!(queue.pop().is_none());
    }

    #[test]
    fn test_clear() {
        let queue = PriorityQueue::new(100);
        let _ = queue.push(PrioritizedOrder::new(
            make_order("1"),
            OrderPriority::Low,
            None,
        ));
        let _ = queue.push(PrioritizedOrder::new(
            make_order("2"),
            OrderPriority::Low,
            None,
        ));

        queue.clear();
        assert!(queue.is_empty());
    }

    #[test]
    fn test_peek_priority() {
        let queue = PriorityQueue::new(100);

        assert!(queue.peek_priority().is_none());

        let _ = queue.push(PrioritizedOrder::new(
            make_order("low"),
            OrderPriority::Low,
            None,
        ));
        assert_eq!(queue.peek_priority(), Some(OrderPriority::Low));

        let _ = queue.push(PrioritizedOrder::new(
            make_order("high"),
            OrderPriority::High,
            None,
        ));
        assert_eq!(queue.peek_priority(), Some(OrderPriority::High));
    }
}
