use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::sync::Mutex;
use std::time::Instant;

use crate::types::{OrderIntent, OrderPriority};

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

    pub fn push(&self, mut order: PrioritizedOrder) -> bool {
        let mut heap = self.heap.lock().unwrap();

        if heap.len() >= self.max_size {
            return false;
        }

        let mut seq = self.sequence_counter.lock().unwrap();
        order.sequence = *seq;
        *seq += 1;

        heap.push(order);
        true
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
        Self::new(1000)
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

        queue.push(PrioritizedOrder::new(
            make_order("low"),
            OrderPriority::Low,
            None,
        ));
        queue.push(PrioritizedOrder::new(
            make_order("high"),
            OrderPriority::High,
            None,
        ));
        queue.push(PrioritizedOrder::new(
            make_order("critical"),
            OrderPriority::Critical,
            None,
        ));
        queue.push(PrioritizedOrder::new(
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

        queue.push(PrioritizedOrder::new(
            make_order("first"),
            OrderPriority::Medium,
            None,
        ));
        queue.push(PrioritizedOrder::new(
            make_order("second"),
            OrderPriority::Medium,
            None,
        ));
        queue.push(PrioritizedOrder::new(
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
        queue.push(PrioritizedOrder::new(
            make_order("expired"),
            OrderPriority::High,
            Some(past),
        ));
        queue.push(PrioritizedOrder::new(
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

        assert!(queue.push(PrioritizedOrder::new(
            make_order("1"),
            OrderPriority::Low,
            None
        )));
        assert!(queue.push(PrioritizedOrder::new(
            make_order("2"),
            OrderPriority::Low,
            None
        )));
        assert!(!queue.push(PrioritizedOrder::new(
            make_order("3"),
            OrderPriority::Low,
            None
        )));

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
        queue.push(PrioritizedOrder::new(
            make_order("1"),
            OrderPriority::Low,
            None,
        ));
        queue.push(PrioritizedOrder::new(
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

        queue.push(PrioritizedOrder::new(
            make_order("low"),
            OrderPriority::Low,
            None,
        ));
        assert_eq!(queue.peek_priority(), Some(OrderPriority::Low));

        queue.push(PrioritizedOrder::new(
            make_order("high"),
            OrderPriority::High,
            None,
        ));
        assert_eq!(queue.peek_priority(), Some(OrderPriority::High));
    }
}
