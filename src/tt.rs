use chess::ChessMove;

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

#[derive(Clone, Copy)]
struct Slot {
    key: u64,
    entry: Entry,
    generation: u8,
}

pub struct Table {
    entries: Vec<Option<Slot>>,
    used: usize,
    generation: u8,
    pub hits: u64,
    pub probes: u64,
    pub cutoffs: u64,
}

impl Table {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: vec![None; capacity],
            used: 0,
            generation: 0,
            hits: 0,
            probes: 0,
            cutoffs: 0,
        }
    }
    pub fn get(&mut self, key: u64) -> Option<Entry> {
        self.probes += 1;
        if self.entries.is_empty() {
            return None;
        }
        let entry = self.entries[(key % self.entries.len() as u64) as usize]
            .filter(|slot| slot.key == key)
            .map(|slot| slot.entry);
        if entry.is_some() {
            self.hits += 1;
        }
        entry
    }
    pub fn hashfull(&self) -> usize {
        self.used.saturating_mul(1000) / self.entries.len().max(1)
    }
    pub fn put(&mut self, key: u64, entry: Entry) {
        if self.entries.is_empty() {
            return;
        }
        let index = (key % self.entries.len() as u64) as usize;
        if let Some(old) = self.entries[index] {
            if old.generation == self.generation && old.entry.depth > entry.depth {
                return;
            }
        } else {
            self.used += 1;
        }
        self.entries[index] = Some(Slot {
            key,
            entry,
            generation: self.generation,
        });
    }
    pub fn next_generation(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }
    pub fn entries_for_megabytes(megabytes: usize) -> usize {
        megabytes.saturating_mul(1024 * 1024) / std::mem::size_of::<Option<Slot>>()
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
    #[test]
    fn collision_and_age_control_replacement() {
        let mut tt = Table::new(1);
        let entry = Entry {
            depth: 5,
            score: 9,
            bound: Bound::Exact,
            best: None,
        };
        tt.put(1, entry);
        tt.put(2, Entry { depth: 2, ..entry });
        assert!(tt.get(2).is_none());
        tt.next_generation();
        tt.put(2, Entry { depth: 2, ..entry });
        assert!(tt.get(1).is_none());
        assert_eq!(tt.get(2).unwrap().score, 9);
    }
}
