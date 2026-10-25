//! Steering and follow-up queues.
//!
//! Steering is injected into the *current* turn, so a user can correct course
//! mid-flight. Follow-ups wait for the turn to settle, so they behave like the
//! next message rather than an interruption.

use std::collections::VecDeque;

#[derive(Debug, Default, Clone)]
pub struct Queues {
    steering: VecDeque<String>,
    follow_up: VecDeque<String>,
}

impl Queues {
    pub fn new() -> Self {
        Queues::default()
    }

    pub fn push_steering(&mut self, text: impl Into<String>) {
        let text = text.into();
        if !text.trim().is_empty() {
            self.steering.push_back(text);
        }
    }

    pub fn push_follow_up(&mut self, text: impl Into<String>) {
        let text = text.into();
        if !text.trim().is_empty() {
            self.follow_up.push_back(text);
        }
    }

    /// Take everything queued for steering, in order.
    pub fn drain_steering(&mut self) -> Vec<String> {
        self.steering.drain(..).collect()
    }

    /// Take the next follow-up, if any.
    pub fn pop_follow_up(&mut self) -> Option<String> {
        self.follow_up.pop_front()
    }

    pub fn steering(&self) -> Vec<String> {
        self.steering.iter().cloned().collect()
    }

    pub fn follow_up(&self) -> Vec<String> {
        self.follow_up.iter().cloned().collect()
    }

    pub fn pending(&self) -> usize {
        self.steering.len() + self.follow_up.len()
    }

    pub fn clear(&mut self) -> (Vec<String>, Vec<String>) {
        let steering = self.drain_steering();
        let follow_up = self.follow_up.drain(..).collect();
        (steering, follow_up)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steering_drains_in_order() {
        let mut queues = Queues::new();
        queues.push_steering("first");
        queues.push_steering("second");
        assert_eq!(queues.drain_steering(), vec!["first", "second"]);
        assert!(queues.steering().is_empty());
    }

    #[test]
    fn follow_ups_are_consumed_one_at_a_time() {
        let mut queues = Queues::new();
        queues.push_follow_up("a");
        queues.push_follow_up("b");
        assert_eq!(queues.pop_follow_up().as_deref(), Some("a"));
        assert_eq!(queues.pop_follow_up().as_deref(), Some("b"));
        assert_eq!(queues.pop_follow_up(), None);
    }

    #[test]
    fn blank_messages_are_ignored() {
        let mut queues = Queues::new();
        queues.push_steering("   ");
        queues.push_follow_up("");
        assert_eq!(queues.pending(), 0);
    }

    #[test]
    fn clearing_returns_both_queues() {
        let mut queues = Queues::new();
        queues.push_steering("s");
        queues.push_follow_up("f");
        let (steering, follow_up) = queues.clear();
        assert_eq!(steering, vec!["s"]);
        assert_eq!(follow_up, vec!["f"]);
        assert_eq!(queues.pending(), 0);
    }
}
