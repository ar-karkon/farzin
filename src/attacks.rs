//! Attack tables. Sliders use build-time generated plain magic bitboards;
//! leapers, between- and line-masks are computed in `const` context.
use crate::types::*;

pub(crate) struct MagicEntry {
    pub mask: u64,
    pub magic: u64,
    pub shift: u32,
    pub offset: usize,
}

include!(concat!(env!("OUT_DIR"), "/magics.rs"));

#[cfg(all(target_arch = "x86_64", target_feature = "bmi2"))]
pub(crate) struct PextEntry {
    pub mask: u64,
    pub offset: usize,
}

#[cfg(all(target_arch = "x86_64", target_feature = "bmi2"))]
include!(concat!(env!("OUT_DIR"), "/pext.rs"));

/// Slider attack lookup method compiled into this binary.
pub const SLIDER_METHOD: &str = if cfg!(all(target_arch = "x86_64", target_feature = "bmi2")) {
    "pext"
} else {
    "magic"
};

#[cfg(all(target_arch = "x86_64", target_feature = "bmi2"))]
#[inline(always)]
fn pext_lookup(entry: &PextEntry, table: &[u64], occ: Bitboard) -> Bitboard {
    // SAFETY: BMI2 is enabled at compile time; the build script sizes each
    // square's slice to 2^popcount(mask), the range of `pext(occ, mask)`.
    unsafe {
        let idx = std::arch::x86_64::_pext_u64(occ, entry.mask) as usize + entry.offset;
        *table.get_unchecked(idx)
    }
}

#[inline(always)]
pub fn rook_attacks(sq: Square, occ: Bitboard) -> Bitboard {
    #[cfg(all(target_arch = "x86_64", target_feature = "bmi2"))]
    {
        pext_lookup(&ROOK_PEXT[sq], &ROOK_PEXT_TABLE, occ)
    }
    #[cfg(not(all(target_arch = "x86_64", target_feature = "bmi2")))]
    {
        rook_attacks_magic(sq, occ)
    }
}

#[inline(always)]
pub fn bishop_attacks(sq: Square, occ: Bitboard) -> Bitboard {
    #[cfg(all(target_arch = "x86_64", target_feature = "bmi2"))]
    {
        pext_lookup(&BISHOP_PEXT[sq], &BISHOP_PEXT_TABLE, occ)
    }
    #[cfg(not(all(target_arch = "x86_64", target_feature = "bmi2")))]
    {
        bishop_attacks_magic(sq, occ)
    }
}

#[inline(always)]
pub fn rook_attacks_magic(sq: Square, occ: Bitboard) -> Bitboard {
    let m = &ROOK_MAGICS[sq];
    let idx = ((occ & m.mask).wrapping_mul(m.magic) >> m.shift) as usize + m.offset;
    // SAFETY: the build script sizes the table to hold every index produced
    // by this square's magic and shift.
    unsafe { *ROOK_TABLE.get_unchecked(idx) }
}

#[inline(always)]
pub fn bishop_attacks_magic(sq: Square, occ: Bitboard) -> Bitboard {
    let m = &BISHOP_MAGICS[sq];
    let idx = ((occ & m.mask).wrapping_mul(m.magic) >> m.shift) as usize + m.offset;
    // SAFETY: as for `rook_attacks_magic`.
    unsafe { *BISHOP_TABLE.get_unchecked(idx) }
}

#[inline(always)]
pub fn queen_attacks(sq: Square, occ: Bitboard) -> Bitboard {
    rook_attacks(sq, occ) | bishop_attacks(sq, occ)
}

const fn leaper_table(deltas: &[(i32, i32)]) -> [Bitboard; 64] {
    let mut table = [0u64; 64];
    let mut sq = 0;
    while sq < 64 {
        let (r, f) = ((sq / 8) as i32, (sq % 8) as i32);
        let mut i = 0;
        while i < deltas.len() {
            let (nr, nf) = (r + deltas[i].0, f + deltas[i].1);
            if nr >= 0 && nr < 8 && nf >= 0 && nf < 8 {
                table[sq] |= 1u64 << (nr * 8 + nf);
            }
            i += 1;
        }
        sq += 1;
    }
    table
}

pub static KNIGHT_ATTACKS: [Bitboard; 64] = leaper_table(&[
    (1, 2),
    (2, 1),
    (2, -1),
    (1, -2),
    (-1, -2),
    (-2, -1),
    (-2, 1),
    (-1, 2),
]);
pub static KING_ATTACKS: [Bitboard; 64] = leaper_table(&[
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
]);
/// `PAWN_ATTACKS[color][sq]`: squares attacked by a pawn of `color` on `sq`.
pub static PAWN_ATTACKS: [[Bitboard; 64]; 2] = [
    leaper_table(&[(1, 1), (1, -1)]),
    leaper_table(&[(-1, 1), (-1, -1)]),
];

const fn ray_tables() -> ([[Bitboard; 64]; 64], [[Bitboard; 64]; 64]) {
    let mut between = [[0u64; 64]; 64];
    let mut line = [[0u64; 64]; 64];
    let dirs = [
        (1, 0),
        (-1, 0),
        (0, 1),
        (0, -1),
        (1, 1),
        (1, -1),
        (-1, 1),
        (-1, -1),
    ];
    let mut a = 0;
    while a < 64 {
        let (r, f) = ((a / 8) as i32, (a % 8) as i32);
        let mut d = 0;
        while d < 8 {
            let (dr, df) = dirs[d];
            // Full line through `a` in this direction pair.
            let mut full = 1u64 << a;
            let mut k = 1;
            while k < 8 {
                let (nr, nf) = (r + dr * k, f + df * k);
                if nr >= 0 && nr < 8 && nf >= 0 && nf < 8 {
                    full |= 1u64 << (nr * 8 + nf);
                }
                let (pr, pf) = (r - dr * k, f - df * k);
                if pr >= 0 && pr < 8 && pf >= 0 && pf < 8 {
                    full |= 1u64 << (pr * 8 + pf);
                }
                k += 1;
            }
            let mut path = 0u64;
            let mut k = 1;
            while k < 8 {
                let (nr, nf) = (r + dr * k, f + df * k);
                if nr < 0 || nr >= 8 || nf < 0 || nf >= 8 {
                    break;
                }
                let b = (nr * 8 + nf) as usize;
                between[a][b] = path;
                line[a][b] = full;
                path |= 1u64 << b;
                k += 1;
            }
            d += 1;
        }
        a += 1;
    }
    (between, line)
}

/// Squares strictly between two aligned squares (empty if not aligned).
pub static BETWEEN: [[Bitboard; 64]; 64] = ray_tables().0;
/// Full board line through two aligned squares (empty if not aligned).
pub static LINE: [[Bitboard; 64]; 64] = ray_tables().1;

#[inline(always)]
pub fn piece_attacks(piece: usize, sq: Square, occ: Bitboard) -> Bitboard {
    match piece {
        KNIGHT => KNIGHT_ATTACKS[sq],
        BISHOP => bishop_attacks(sq, occ),
        ROOK => rook_attacks(sq, occ),
        QUEEN => queen_attacks(sq, occ),
        KING => KING_ATTACKS[sq],
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PEXT (when compiled in) and magic lookups agree on random occupancies.
    #[test]
    fn slider_methods_agree() {
        let mut x = 0x2545_F491_4F6C_DD1Du64;
        for _ in 0..20_000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let occ = x & (x >> 3);
            let sq = (x >> 58) as usize;
            assert_eq!(rook_attacks(sq, occ), rook_attacks_magic(sq, occ));
            assert_eq!(bishop_attacks(sq, occ), bishop_attacks_magic(sq, occ));
        }
    }
}
