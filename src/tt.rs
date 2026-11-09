use chess::ChessMove;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bound {
    Exact,
    Lower,
    Upper,
}

#[derive(Clone, Copy)]
pub struct Entry {
    pub depth: u8,
    pub score: i32,
    pub bound: Bound,
    pub best: Option<ChessMove>,
}

pub struct Table {
    entries: HashMap<u64, Entry>,
    capacity: usize,
    pub hits: u64,
    pub cutoffs: u64,
}

impl Table {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            capacity,
            hits: 0,
            cutoffs: 0,
        }
    }
    pub fn get(&mut self, key: u64) -> Option<Entry> {
        let entry = self.entries.get(&key).copied();
        if entry.is_some() {
            self.hits += 1;
        }
        entry
    }
    pub fn hashfull(&self) -> usize {
        self.entries.len().saturating_mul(1000) / self.capacity.max(1)
    }
    pub fn put(&mut self, key: u64, entry: Entry) {
        if let Some(old) = self.entries.get(&key) {
            if old.depth > entry.depth {
                return;
            }
        } else if self.entries.len() >= self.capacity {
            return;
        }
        self.entries.insert(key, entry);
    }
}

pub fn encode_mate(score: i32, ply: i32) -> i32 {
    if score > 29_900 {
        score + ply
    } else if score < -29_900 {
        score - ply
    } else {
        score
    }
}
pub fn decode_mate(score: i32, ply: i32) -> i32 {
    if score > 29_900 {
        score - ply
    } else if score < -29_900 {
        score + ply
    } else {
        score
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mate_distance_adjusts_to_new_ply() {
        assert_eq!(decode_mate(encode_mate(29_995, 3), 7), 29_991);
        assert_eq!(decode_mate(encode_mate(-29_995, 3), 7), -29_991);
        assert_eq!(decode_mate(encode_mate(100, 3), 7), 100);
    }
    #[test]
    fn deeper_entry_is_preserved() {
        let mut tt = Table::new(1);
        tt.put(
            1,
            Entry {
                depth: 5,
                score: 9,
                bound: Bound::Exact,
                best: None,
            },
        );
        tt.put(
            1,
            Entry {
                depth: 2,
                score: 1,
                bound: Bound::Upper,
                best: None,
            },
        );
        assert_eq!(tt.get(1).unwrap().score, 9);
    }
}
