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

#[inline(always)]
pub fn rook_attacks(sq: Square, occ: Bitboard) -> Bitboard {
    let m = &ROOK_MAGICS[sq];
    let idx = ((occ & m.mask).wrapping_mul(m.magic) >> m.shift) as usize + m.offset;
    // SAFETY: the build script sizes the table to hold every index produced
    // by this square's magic and shift.
    unsafe { *ROOK_TABLE.get_unchecked(idx) }
}

#[inline(always)]
pub fn bishop_attacks(sq: Square, occ: Bitboard) -> Bitboard {
    let m = &BISHOP_MAGICS[sq];
    let idx = ((occ & m.mask).wrapping_mul(m.magic) >> m.shift) as usize + m.offset;
    // SAFETY: as for `rook_attacks`.
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

const RAYS: ([[Bitboard; 64]; 64], [[Bitboard; 64]; 64]) = ray_tables();
/// Squares strictly between two aligned squares (empty if not aligned).
pub static BETWEEN: [[Bitboard; 64]; 64] = RAYS.0;
/// Full board line through two aligned squares (empty if not aligned).
pub static LINE: [[Bitboard; 64]; 64] = RAYS.1;

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
