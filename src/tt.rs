//! Shared, lock-free transposition table (XOR-validated entries, 4 per line).
use crate::types::Move;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};

pub const BOUND_NONE: u8 = 0;
pub const BOUND_UPPER: u8 = 1;
pub const BOUND_LOWER: u8 = 2;
pub const BOUND_EXACT: u8 = 3;

pub const MATE: i32 = 32_000;
pub const MATE_BOUND: i32 = MATE - 512;
pub const INF: i32 = 32_500;
pub const NO_SCORE: i32 = -32_700;

#[derive(Clone, Copy, Debug)]
pub struct TtData {
    pub mv: Move,
    pub score: i32,
    pub eval: i32,
    pub depth: i32,
    pub bound: u8,
    pub pv: bool,
}

#[derive(Default)]
struct Entry {
    key: AtomicU64,
    data: AtomicU64,
}

#[repr(align(64))]
#[derive(Default)]
struct Cluster {
    entries: [Entry; 4],
}

pub struct TranspositionTable {
    clusters: Vec<Cluster>,
    age: AtomicU8,
}

#[inline]
fn pack(mv: Move, score: i32, eval: i32, depth: i32, bound: u8, pv: bool, age: u8) -> u64 {
    u64::from(mv.0)
        | (u64::from(score as i16 as u16) << 16)
        | (u64::from(eval as i16 as u16) << 32)
        | (u64::from(depth.clamp(0, 255) as u8) << 48)
        | (u64::from(bound | (u8::from(pv) << 2) | (age << 3)) << 56)
}

#[inline]
fn unpack(data: u64) -> (TtData, u8) {
    let meta = (data >> 56) as u8;
    (
        TtData {
            mv: Move(data as u16),
            score: i32::from((data >> 16) as u16 as i16),
            eval: i32::from((data >> 32) as u16 as i16),
            depth: i32::from((data >> 48) as u8),
            bound: meta & 3,
            pv: meta & 4 != 0,
        },
        meta >> 3,
    )
}

pub fn score_to_tt(score: i32, ply: usize) -> i32 {
    if score >= MATE_BOUND {
        score + ply as i32
    } else if score <= -MATE_BOUND {
        score - ply as i32
    } else {
        score
    }
}

pub fn score_from_tt(score: i32, ply: usize, halfmove: u16) -> i32 {
    if score == NO_SCORE {
        NO_SCORE
    } else if score >= MATE_BOUND {
        // A mate found beyond the 50-move horizon may not be reachable.
        if MATE - score > 100 - i32::from(halfmove) {
            return MATE_BOUND - 1;
        }
        score - ply as i32
    } else if score <= -MATE_BOUND {
        if MATE + score > 100 - i32::from(halfmove) {
            return -MATE_BOUND + 1;
        }
        score + ply as i32
    } else {
        score
    }
}

impl TranspositionTable {
    pub fn new(mb: usize) -> Self {
        let count = (mb.max(1) * 1024 * 1024 / std::mem::size_of::<Cluster>()).max(1);
        let mut clusters = Vec::with_capacity(count);
        clusters.resize_with(count, Cluster::default);
        TranspositionTable {
            clusters,
            age: AtomicU8::new(0),
        }
    }

    pub fn clear(&mut self) {
        for c in &mut self.clusters {
            for e in &mut c.entries {
                *e.key.get_mut() = 0;
                *e.data.get_mut() = 0;
            }
        }
        *self.age.get_mut() = 0;
    }

    pub fn new_search(&self) {
        let age = (self.age.load(Ordering::Relaxed) + 1) & 31;
        self.age.store(age, Ordering::Relaxed);
    }

    #[inline]
    fn cluster(&self, hash: u64) -> &Cluster {
        let idx = ((u128::from(hash) * self.clusters.len() as u128) >> 64) as usize;
        // SAFETY: idx < len by construction of the multiply-shift.
        unsafe { self.clusters.get_unchecked(idx) }
    }

    #[inline]
    pub fn prefetch(&self, hash: u64) {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            use std::arch::x86_64::{_mm_prefetch, _MM_HINT_T0};
            _mm_prefetch::<_MM_HINT_T0>(self.cluster(hash) as *const Cluster as *const i8);
        }
        #[cfg(not(target_arch = "x86_64"))]
        let _ = hash;
    }

    pub fn probe(&self, hash: u64) -> Option<TtData> {
        for e in &self.cluster(hash).entries {
            let data = e.data.load(Ordering::Relaxed);
            if e.key.load(Ordering::Relaxed) ^ data == hash && data != 0 {
                return Some(unpack(data).0);
            }
        }
        None
    }

    #[allow(clippy::too_many_arguments)]
    pub fn store(
        &self,
        hash: u64,
        mv: Move,
        score: i32,
        eval: i32,
        depth: i32,
        bound: u8,
        pv: bool,
    ) {
        let age = self.age.load(Ordering::Relaxed);
        let cluster = self.cluster(hash);
        let mut replace = &cluster.entries[0];
        let mut worst = i32::MAX;
        for e in &cluster.entries {
            let data = e.data.load(Ordering::Relaxed);
            let key = e.key.load(Ordering::Relaxed) ^ data;
            if key == hash || data == 0 {
                replace = e;
                break;
            }
            let (old, old_age) = unpack(data);
            let age_diff = i32::from((32 + age - old_age) & 31);
            let value = old.depth - 8 * age_diff;
            if value < worst {
                worst = value;
                replace = e;
            }
        }
        let old_data = replace.data.load(Ordering::Relaxed);
        let same = replace.key.load(Ordering::Relaxed) ^ old_data == hash && old_data != 0;
        let (old, _) = unpack(old_data);
        // Keep a deeper entry for the same position unless the new one is exact.
        if same && bound != BOUND_EXACT && depth + 4 < old.depth {
            return;
        }
        let mv = if mv.is_none() && same { old.mv } else { mv };
        let data = pack(mv, score, eval, depth, bound, pv, age);
        replace.key.store(hash ^ data, Ordering::Relaxed);
        replace.data.store(data, Ordering::Relaxed);
    }

    /// Permille of sampled entries written during the current search.
    pub fn hashfull(&self) -> usize {
        let age = self.age.load(Ordering::Relaxed);
        let sample = self.clusters.len().min(250);
        let mut used = 0;
        for c in &self.clusters[..sample] {
            for e in &c.entries {
                let data = e.data.load(Ordering::Relaxed);
                if data != 0 && unpack(data).1 == age {
                    used += 1;
                }
            }
        }
        used * 1000 / (sample * 4)
    }
}
