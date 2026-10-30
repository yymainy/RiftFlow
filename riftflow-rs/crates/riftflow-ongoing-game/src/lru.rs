//! `quick-lru` — the two 256-entry caches the match-history loader keeps, so a
//! game summary or timeline already fetched for another player is not fetched twice.

use std::collections::HashMap;

#[derive(Clone, Debug)]
struct Entry {
    value: serde_json::Value,
    /// the touch counter at the time it was last read or written
    used: u64,
}

pub struct Lru {
    max_size: usize,
    entries: HashMap<String, Entry>,
    touched: u64,
}

impl Lru {
    pub fn new(max_size: usize) -> Self {
        Self { max_size, entries: HashMap::new(), touched: 0 }
    }

    pub fn get(&mut self, key: &str) -> Option<serde_json::Value> {
        let entry = self.entries.get_mut(key)?;
        self.touched += 1;
        entry.used = self.touched;
        Some(entry.value.clone())
    }

    pub fn contains(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }

    pub fn set(&mut self, key: &str, value: serde_json::Value) {
        self.touched += 1;
        self.entries.insert(key.to_string(), Entry { value, used: self.touched });

        // quick-lru evicts the least recently used entries in one pass
        while self.entries.len() > self.max_size {
            let victim = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| key.clone());

            match victim {
                Some(key) => {
                    self.entries.remove(&key);
                }
                None => break,
            }
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_read_protects_an_entry_from_the_next_eviction() {
        let mut lru = Lru::new(2);
        lru.set("a", json!(1));
        lru.set("b", json!(2));
        assert_eq!(lru.get("a"), Some(json!(1)));
        lru.set("c", json!(3));

        assert!(lru.contains("a"), "`a` was just read");
        assert!(!lru.contains("b"), "`b` is the least recently used");
        assert_eq!(lru.len(), 2);
    }

    #[test]
    fn writing_the_same_key_twice_updates_in_place() {
        let mut lru = Lru::new(2);
        lru.set("a", json!(1));
        lru.set("a", json!(2));

        assert_eq!(lru.len(), 1);
        assert_eq!(lru.get("a"), Some(json!(2)));
    }

    #[test]
    fn a_miss_reads_as_absent_and_eviction_is_bounded() {
        let mut lru = Lru::new(1);
        assert_eq!(lru.get("nope"), None);

        for index in 0..50 {
            lru.set(&index.to_string(), json!(index));
        }

        assert_eq!(lru.len(), 1);
        assert_eq!(lru.get("49"), Some(json!(49)));
    }
}
