//! Undo/redo for edit state. Each push stores a full snapshot (edit state is small), and pushes
//! that share a coalescing key within the same drag replace the last entry, so a slider drag is
//! one undo step. Named snapshots are kept separately and do not change the undo stack.

use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct History<T: Clone> {
    past: Vec<(Option<&'static str>, T)>,
    future: Vec<T>,
    current: T,
    /// Named snapshots, sorted by name, so saves are deterministic.
    snapshots: BTreeMap<String, T>,
    /// Maximum undo depth. Older entries are dropped.
    limit: usize,
}

impl<T: Clone + PartialEq> History<T> {
    pub fn new(initial: T) -> Self {
        Self { past: Vec::new(), future: Vec::new(), current: initial, snapshots: BTreeMap::new(), limit: 200 }
    }

    pub fn current(&self) -> &T {
        &self.current
    }

    /// Records a new state. Identical states are ignored. If `key` matches the previous push and
    /// that push was not followed by an undo, the new state replaces it, so a continuous drag
    /// (same key on every move) becomes one step.
    pub fn push(&mut self, key: Option<&'static str>, state: T) {
        if state == self.current {
            return;
        }
        let coalesce = key.is_some() && self.past.last().map(|(k, _)| *k == key).unwrap_or(false) && self.future.is_empty();
        if !coalesce {
            self.past.push((key, self.current.clone()));
            if self.past.len() > self.limit {
                self.past.remove(0);
            }
        }
        self.current = state;
        self.future.clear();
    }

    pub fn undo(&mut self) -> bool {
        match self.past.pop() {
            Some((_, prev)) => {
                self.future.push(std::mem::replace(&mut self.current, prev));
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        match self.future.pop() {
            Some(next) => {
                self.past.push((None, std::mem::replace(&mut self.current, next)));
                true
            }
            None => false,
        }
    }

    pub fn save_snapshot(&mut self, name: &str) {
        self.snapshots.insert(name.to_string(), self.current.clone());
    }

    /// Restores a snapshot as a new step, so it can be undone.
    pub fn restore_snapshot(&mut self, name: &str) -> bool {
        match self.snapshots.get(name).cloned() {
            Some(state) => {
                self.push(None, state);
                true
            }
            None => false,
        }
    }

    pub fn snapshot_names(&self) -> Vec<&str> {
        self.snapshots.keys().map(String::as_str).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_and_redo_walk_the_states() {
        let mut h = History::new(0.0f32);
        h.push(None, 1.0);
        h.push(None, 2.0);
        assert!(h.undo());
        assert_eq!(*h.current(), 1.0);
        assert!(h.redo());
        assert_eq!(*h.current(), 2.0);
        assert!(!h.redo(), "nothing to redo after the last state");
    }

    #[test]
    fn a_drag_with_one_key_is_a_single_step() {
        let mut h = History::new(0.0f32);
        for v in [0.1, 0.2, 0.3, 0.4, 0.5] {
            h.push(Some("strength"), v);
        }
        assert!(h.undo());
        assert_eq!(*h.current(), 0.0, "the whole drag should undo at once");
        assert!(!h.undo());
    }

    #[test]
    fn a_new_key_starts_a_new_step() {
        let mut h = History::new(0.0f32);
        h.push(Some("strength"), 0.5);
        h.push(Some("warmth"), 0.2);
        assert!(h.undo());
        assert_eq!(*h.current(), 0.5);
    }

    #[test]
    fn snapshots_restore_as_an_undoable_step() {
        let mut h = History::new(1.0f32);
        h.save_snapshot("good");
        h.push(None, 5.0);
        assert!(h.restore_snapshot("good"));
        assert_eq!(*h.current(), 1.0);
        assert!(h.undo());
        assert_eq!(*h.current(), 5.0);
        assert_eq!(h.snapshot_names(), vec!["good"]);
    }

    #[test]
    fn identical_states_are_not_recorded() {
        let mut h = History::new(3u32);
        h.push(None, 3);
        assert!(!h.undo());
    }
}
