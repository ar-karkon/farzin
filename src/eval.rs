//! Hand-crafted tapered evaluation (bootstrap evaluator; NNUE replaces it
//! when a network is loaded). All values are Farzin's own.
use crate::attacks::*;
use crate::board::Board;
use crate::types::*;

#[derive(Clone, Copy)]
struct S(i32, i32);

impl std::ops::Add for S {
    type Output = S;
    fn add(self, o: S) -> S {
        S(self.0 + o.0, self.1 + o.1)
    }
}
impl std::ops::Sub for S {
    type Output = S;
    fn sub(self, o: S) -> S {
        S(self.0 - o.0, self.1 - o.1)
    }
}
impl std::ops::AddAssign for S {
    fn add_assign(&mut self, o: S) {
        self.0 += o.0;
        self.1 += o.1;
    }
}
impl std::ops::Mul<i32> for S {
    type Output = S;
    fn mul(self, k: i32) -> S {
        S(self.0 * k, self.1 * k)
    }
}

const MATERIAL: [S; 6] = [
    S(85, 110),
    S(330, 300),
    S(350, 320),
    S(470, 540),
    S(1000, 980),
    S(0, 0),
];

pub const PHASE_WEIGHT: [i32; 6] = [0, 1, 1, 2, 4, 0];

// Piece-square tables from White's point of view, index a1 = 0 .. h8 = 63
// (so the first row printed is rank 1).
#[rustfmt::skip]
const PAWN_MG: [i32; 64] = [
      0,   0,   0,   0,   0,   0,   0,   0,
     -5,   0,   0, -10, -10,   5,  10,  -5,
     -5,   0,   5,   5,   5,   0,   5,  -5,
     -5,   0,  10,  20,  20,   5,   0,  -8,
      0,   5,  12,  25,  25,  10,   5,   0,
     10,  15,  25,  35,  35,  25,  15,  10,
     50,  50,  50,  55,  55,  50,  50,  50,
      0,   0,   0,   0,   0,   0,   0,   0,
];
#[rustfmt::skip]
const PAWN_EG: [i32; 64] = [
      0,   0,   0,   0,   0,   0,   0,   0,
      5,   5,   5,   5,   5,   5,   5,   5,
      0,   0,   0,   0,   0,   0,   0,   0,
      5,   5,   0,   0,   0,   0,   5,   5,
     15,  15,  10,  10,  10,  10,  15,  15,
     40,  40,  35,  30,  30,  35,  40,  40,
     80,  80,  75,  70,  70,  75,  80,  80,
      0,   0,   0,   0,   0,   0,   0,   0,
];
#[rustfmt::skip]
const KNIGHT_MG: [i32; 64] = [
    -60, -25, -20, -15, -15, -20, -25, -60,
    -25, -15,   0,   5,   5,   0, -15, -25,
    -20,   0,  10,  12,  12,  10,   0, -20,
    -15,   5,  15,  20,  20,  15,   5, -15,
    -10,  10,  20,  25,  25,  20,  10, -10,
    -15,  10,  20,  25,  25,  20,  10, -15,
    -25, -10,   5,  10,  10,   5, -10, -25,
    -70, -30, -20, -15, -15, -20, -30, -70,
];
#[rustfmt::skip]
const KNIGHT_EG: [i32; 64] = [
    -40, -25, -15, -10, -10, -15, -25, -40,
    -25, -10,   0,   5,   5,   0, -10, -25,
    -15,   0,  10,  15,  15,  10,   0, -15,
    -10,   5,  15,  20,  20,  15,   5, -10,
    -10,   5,  15,  20,  20,  15,   5, -10,
    -15,   0,  10,  15,  15,  10,   0, -15,
    -25, -10,   0,   5,   5,   0, -10, -25,
    -40, -25, -15, -10, -10, -15, -25, -40,
];
#[rustfmt::skip]
const BISHOP_MG: [i32; 64] = [
    -15,  -5, -10,  -5,  -5, -10,  -5, -15,
      0,  10,   5,   5,   5,   5,  10,   0,
     -5,   5,  10,   8,   8,  10,   5,  -5,
     -5,   5,  10,  15,  15,  10,   5,  -5,
     -5,   8,  10,  15,  15,  10,   8,  -5,
     -5,   5,  10,  10,  10,  10,   5,  -5,
    -10,   0,   0,   0,   0,   0,   0, -10,
    -20, -10, -10, -10, -10, -10, -10, -20,
];
#[rustfmt::skip]
const BISHOP_EG: [i32; 64] = [
    -15, -10,  -5,  -5,  -5,  -5, -10, -15,
    -10,  -5,   0,   0,   0,   0,  -5, -10,
     -5,   0,   5,   5,   5,   5,   0,  -5,
     -5,   0,   5,  10,  10,   5,   0,  -5,
     -5,   0,   5,  10,  10,   5,   0,  -5,
     -5,   0,   5,   5,   5,   5,   0,  -5,
    -10,  -5,   0,   0,   0,   0,  -5, -10,
    -15, -10,  -5,  -5,  -5,  -5, -10, -15,
];
#[rustfmt::skip]
const ROOK_MG: [i32; 64] = [
     -5,   0,   5,  10,  10,   5,   0,  -5,
    -15,  -5,   0,   0,   0,   0,  -5, -15,
    -15,  -5,   0,   0,   0,   0,  -5, -15,
    -15,  -5,   0,   0,   0,   0,  -5, -15,
    -10,  -5,   0,   5,   5,   0,  -5, -10,
     -5,   5,   5,  10,  10,   5,   5,  -5,
     15,  20,  20,  20,  20,  20,  20,  15,
      5,   5,   5,  10,  10,   5,   5,   5,
];
#[rustfmt::skip]
const ROOK_EG: [i32; 64] = [
     -5,  -5,   0,   0,   0,   0,  -5,  -5,
     -5,  -5,   0,   0,   0,   0,  -5,  -5,
     -5,   0,   0,   0,   0,   0,   0,  -5,
      0,   0,   5,   5,   5,   5,   0,   0,
      5,   5,   5,   5,   5,   5,   5,   5,
      5,   5,   5,   5,   5,   5,   5,   5,
     10,  10,  10,  10,  10,  10,  10,  10,
      5,   5,   5,   5,   5,   5,   5,   5,
];
#[rustfmt::skip]
const QUEEN_MG: [i32; 64] = [
    -15, -10,  -5,   0,  -5, -10, -10, -15,
    -10,   0,   5,   5,   5,   0,   0, -10,
     -5,   0,   5,   5,   5,   5,   0,  -5,
     -5,   0,   5,   5,   5,   5,   0,  -5,
     -5,   0,   5,   5,   5,   5,   0,  -5,
     -5,   0,   5,   5,   5,   5,   0,  -5,
    -10,  -5,   0,   0,   0,   0,  -5, -10,
    -15, -10,  -5,  -5,  -5,  -5, -10, -15,
];
#[rustfmt::skip]
const QUEEN_EG: [i32; 64] = [
    -25, -15, -10,  -5,  -5, -10, -15, -25,
    -15,  -5,   0,   5,   5,   0,  -5, -15,
    -10,   0,  10,  15,  15,  10,   0, -10,
     -5,   5,  15,  20,  20,  15,   5,  -5,
     -5,   5,  15,  20,  20,  15,   5,  -5,
    -10,   0,  10,  15,  15,  10,   0, -10,
    -15,  -5,   0,   5,   5,   0,  -5, -15,
    -25, -15, -10,  -5,  -5, -10, -15, -25,
];
#[rustfmt::skip]
const KING_MG: [i32; 64] = [
     20,  35,  15, -15,   0, -10,  35,  25,
     10,  10,  -5, -25, -25, -15,  10,  10,
    -15, -20, -30, -40, -40, -30, -20, -15,
    -30, -40, -50, -60, -60, -50, -40, -30,
    -40, -50, -60, -70, -70, -60, -50, -40,
    -50, -60, -70, -80, -80, -70, -60, -50,
    -60, -70, -80, -90, -90, -80, -70, -60,
    -70, -80, -90, -99, -99, -90, -80, -70,
];
#[rustfmt::skip]
const KING_EG: [i32; 64] = [
    -50, -30, -20, -15, -15, -20, -30, -50,
    -30, -10,   0,   5,   5,   0, -10, -30,
    -20,   0,  15,  20,  20,  15,   0, -20,
    -15,   5,  20,  30,  30,  20,   5, -15,
    -15,   5,  20,  30,  30,  20,   5, -15,
    -20,   0,  15,  25,  25,  15,   0, -20,
    -30, -10,   5,  10,  10,   5, -10, -30,
    -50, -30, -20, -15, -15, -20, -30, -50,
];

const fn build_pst() -> [[[i32; 64]; 2]; 6] {
    let tables: [(&[i32; 64], &[i32; 64]); 6] = [
        (&PAWN_MG, &PAWN_EG),
        (&KNIGHT_MG, &KNIGHT_EG),
        (&BISHOP_MG, &BISHOP_EG),
        (&ROOK_MG, &ROOK_EG),
        (&QUEEN_MG, &QUEEN_EG),
        (&KING_MG, &KING_EG),
    ];
    let mut out = [[[0; 64]; 2]; 6];
    let mut p = 0;
    while p < 6 {
        let mut sq = 0;
        while sq < 64 {
            out[p][0][sq] = MATERIAL_MG[p] + tables[p].0[sq];
            out[p][1][sq] = MATERIAL_EG[p] + tables[p].1[sq];
            sq += 1;
        }
        p += 1;
    }
    out
}
const MATERIAL_MG: [i32; 6] = [85, 330, 350, 470, 1000, 0];
const MATERIAL_EG: [i32; 6] = [110, 300, 320, 540, 980, 0];
/// `PST[piece][mg|eg][white-relative square]`, material included.
static PST: [[[i32; 64]; 2]; 6] = build_pst();

const MOBILITY: [S; 6] = [S(0, 0), S(5, 5), S(5, 5), S(3, 5), S(2, 3), S(0, 0)];
const MOBILITY_CENTER: [i32; 6] = [0, 4, 6, 6, 12, 0];
const PASSED_MG: [i32; 8] = [0, 0, 5, 10, 25, 45, 75, 0];
const PASSED_EG: [i32; 8] = [0, 10, 15, 30, 55, 95, 150, 0];
const DOUBLED: S = S(-10, -22);
const ISOLATED: S = S(-12, -12);
const BACKWARD: S = S(-6, -8);
const PHALANX: S = S(6, 8);
const SUPPORTED: S = S(8, 6);
const BISHOP_PAIR: S = S(30, 55);
const ROOK_OPEN: S = S(28, 10);
const ROOK_SEMI_OPEN: S = S(12, 8);
const KNIGHT_OUTPOST: S = S(20, 12);
const SHIELD: S = S(10, 0);
const THREAT_BY_PAWN: S = S(50, 30);
const THREAT_BY_MINOR: S = S(25, 25);
const HANGING: S = S(30, 15);
const TEMPO: i32 = 15;
const KING_ATTACK_WEIGHT: [i32; 6] = [0, 20, 20, 40, 80, 0];

const fn adjacent_files() -> [Bitboard; 8] {
    let mut out = [0; 8];
    let mut f = 0;
    while f < 8 {
        if f > 0 {
            out[f] |= FILE_A << (f - 1);
        }
        if f < 7 {
            out[f] |= FILE_A << (f + 1);
        }
        f += 1;
    }
    out
}
static ADJACENT: [Bitboard; 8] = adjacent_files();

/// Squares in front of `sq` (from `color`'s view) on its file and the
/// adjacent files: a pawn is passed when no enemy pawn stands there.
const fn passed_masks() -> [[Bitboard; 64]; 2] {
    let mut out = [[0; 64]; 2];
    let mut sq = 0;
    while sq < 64 {
        let f = sq % 8;
        let r = sq / 8;
        let mut files = FILE_A << f;
        if f > 0 {
            files |= FILE_A << (f - 1);
        }
        if f < 7 {
            files |= FILE_A << (f + 1);
        }
        let ahead_white = if r < 7 { !0u64 << ((r + 1) * 8) } else { 0 };
        let ahead_black = if r > 0 { !0u64 >> ((8 - r) * 8) } else { 0 };
        out[0][sq] = files & ahead_white;
        out[1][sq] = files & ahead_black;
        sq += 1;
    }
    out
}
static PASSED_MASK: [[Bitboard; 64]; 2] = passed_masks();

#[inline]
fn pawn_attacks_bb(pawns: Bitboard, color: usize) -> Bitboard {
    if color == WHITE {
        ((pawns & !FILE_A) << 7) | ((pawns & !FILE_H) << 9)
    } else {
        ((pawns & !FILE_A) >> 9) | ((pawns & !FILE_H) >> 7)
    }
}

#[inline(always)]
fn rel(sq: Square, color: usize) -> Square {
    if color == WHITE {
        sq
    } else {
        flip(sq)
    }
}

pub fn game_phase(board: &Board) -> i32 {
    let mut phase = 0;
    for p in KNIGHT..=QUEEN {
        phase += board.pieces[p].count_ones() as i32 * PHASE_WEIGHT[p];
    }
    phase.min(24)
}

fn evaluate_side(board: &Board, us: usize, attacks: &[Bitboard; 2]) -> S {
    let them = us ^ 1;
    let occ = board.occupied();
    let own_pawns = board.piece_bb(us, PAWN);
    let enemy_pawns = board.piece_bb(them, PAWN);
    let enemy_pawn_attacks = pawn_attacks_bb(enemy_pawns, them);
    let own_pawn_attacks = pawn_attacks_bb(own_pawns, us);
    let mobility_area =
        !(board.colors[us] & (board.pieces[KING] | own_pawns)) & !enemy_pawn_attacks;
    let enemy_king = board.king_sq(them);
    let king_zone = KING_ATTACKS[enemy_king] | bit(enemy_king);
    let mut score = S(0, 0);
    let mut attack_weight = 0;
    let mut attackers = 0;

    for piece in PAWN..=KING {
        let mut bb = board.piece_bb(us, piece);
        while bb != 0 {
            let sq = pop_lsb(&mut bb);
            let r = rel(sq, us);
            score += S(PST[piece][0][r], PST[piece][1][r]);
            if piece == PAWN || piece == KING {
                continue;
            }
            let att = match piece {
                BISHOP => bishop_attacks(sq, occ ^ board.piece_bb(us, QUEEN)),
                ROOK => rook_attacks(
                    sq,
                    occ ^ board.piece_bb(us, QUEEN) ^ board.piece_bb(us, ROOK),
                ),
                _ => piece_attacks(piece, sq, occ),
            };
            let mob = (att & mobility_area).count_ones() as i32;
            score += MOBILITY[piece] * (mob - MOBILITY_CENTER[piece]);
            let zone_hits = (att & king_zone).count_ones() as i32;
            if zone_hits > 0 {
                attackers += 1;
                attack_weight += KING_ATTACK_WEIGHT[piece] * zone_hits;
            }
            let file = FILE_A << file_of(sq);
            if piece == ROOK {
                if file & own_pawns == 0 {
                    score += if file & enemy_pawns == 0 {
                        ROOK_OPEN
                    } else {
                        ROOK_SEMI_OPEN
                    };
                }
            } else if piece == KNIGHT {
                let outpost_ranks = if us == WHITE {
                    RANK_4 | RANK_5 | (RANK_1 << 40)
                } else {
                    RANK_4 | RANK_5 | (RANK_1 << 16)
                };
                if bit(sq) & outpost_ranks & own_pawn_attacks != 0
                    && PASSED_MASK[us][sq] & ADJACENT[file_of(sq)] & enemy_pawns == 0
                {
                    score += KNIGHT_OUTPOST;
                }
            }
        }
    }

    if board.piece_bb(us, BISHOP).count_ones() >= 2 {
        score += BISHOP_PAIR;
    }

    // Pawn structure.
    let mut pawns = own_pawns;
    while pawns != 0 {
        let sq = pop_lsb(&mut pawns);
        let f = file_of(sq);
        let file = FILE_A << f;
        let r = rank_of(rel(sq, us));
        if ADJACENT[f] & own_pawns == 0 {
            score += ISOLATED;
        } else {
            let rank_bb = RANK_1 << (8 * rank_of(sq));
            if ADJACENT[f] & own_pawns & rank_bb != 0 {
                score += PHALANX;
            }
            if PAWN_ATTACKS[them][sq] & own_pawns != 0 {
                score += SUPPORTED;
            } else {
                // Backward: no friendly pawn behind on adjacent files and the
                // stop square is attacked by an enemy pawn.
                let stop = if us == WHITE { sq + 8 } else { sq - 8 };
                let behind = PASSED_MASK[them][sq] & ADJACENT[f];
                if stop < 64 && behind & own_pawns == 0 && enemy_pawn_attacks & bit(stop) != 0 {
                    score += BACKWARD;
                }
            }
        }
        if (PASSED_MASK[us][sq] & file & own_pawns) != 0 {
            score += DOUBLED;
        }
        if PASSED_MASK[us][sq] & enemy_pawns == 0 && (PASSED_MASK[us][sq] & file & own_pawns) == 0 {
            let mut bonus = S(PASSED_MG[r], PASSED_EG[r]);
            let stop = if us == WHITE {
                sq + 8
            } else {
                sq.wrapping_sub(8)
            };
            if stop < 64 {
                if occ & bit(stop) == 0 {
                    bonus += S(r as i32 * 2, r as i32 * 5);
                }
                if attacks[them] & bit(stop) == 0 {
                    bonus += S(0, r as i32 * 4);
                }
                // King proximity in the endgame.
                let dist = |a: Square, b: Square| {
                    (rank_of(a) as i32 - rank_of(b) as i32)
                        .abs()
                        .max((file_of(a) as i32 - file_of(b) as i32).abs())
                };
                let weight = (r as i32 - 2).max(0);
                bonus += S(
                    0,
                    weight * (5 * dist(stop, enemy_king) - 2 * dist(stop, board.king_sq(us))),
                );
            }
            score += bonus;
        }
    }

    // Pawn shield in front of our king.
    let king = board.king_sq(us);
    let shield_zone = if us == WHITE {
        (KING_ATTACKS[king] | bit(king)) << 8
    } else {
        (KING_ATTACKS[king] | bit(king)) >> 8
    };
    score += SHIELD * (shield_zone & own_pawns).count_ones().min(3) as i32;

    // Attacks against the enemy king.
    if attackers >= 2 || board.piece_bb(us, QUEEN) != 0 && attackers >= 1 {
        let danger = attack_weight * attackers.min(4) / 4;
        let mg = danger * danger / 120 + danger / 2;
        score += S(mg.min(600), mg.min(600) / 6);
    }

    // Threats.
    let enemy_pieces = board.colors[them] & !board.pieces[PAWN] & !board.pieces[KING];
    score += THREAT_BY_PAWN * (own_pawn_attacks & enemy_pieces).count_ones() as i32;
    let mut minor_att = 0;
    for p in [KNIGHT, BISHOP] {
        let mut bb = board.piece_bb(us, p);
        while bb != 0 {
            minor_att |= piece_attacks(p, pop_lsb(&mut bb), occ);
        }
    }
    let majors = board.colors[them] & (board.pieces[ROOK] | board.pieces[QUEEN]);
    score += THREAT_BY_MINOR * (minor_att & majors).count_ones() as i32;
    let hanging = enemy_pieces & attacks[us] & !attacks[them];
    score += HANGING * hanging.count_ones() as i32;
    score
}

fn all_attacks(board: &Board, color: usize) -> Bitboard {
    let occ = board.occupied();
    let mut att = pawn_attacks_bb(board.piece_bb(color, PAWN), color);
    for piece in KNIGHT..=KING {
        let mut bb = board.piece_bb(color, piece);
        while bb != 0 {
            att |= piece_attacks(piece, pop_lsb(&mut bb), occ);
        }
    }
    att
}

/// Scale factor (out of 128) for drawish material configurations.
fn scale_factor(board: &Board, strong: usize) -> i32 {
    let weak = strong ^ 1;
    let strong_pawns = board.piece_bb(strong, PAWN).count_ones();
    let minor_major = |c: usize| {
        board.piece_bb(c, KNIGHT).count_ones() as i32 * 3
            + board.piece_bb(c, BISHOP).count_ones() as i32 * 3
            + board.piece_bb(c, ROOK).count_ones() as i32 * 5
            + board.piece_bb(c, QUEEN).count_ones() as i32 * 9
    };
    let (s, w) = (minor_major(strong), minor_major(weak));
    if strong_pawns == 0 && s - w <= 3 {
        return if s <= 3 { 0 } else { 16 };
    }
    // Opposite-coloured bishops with no other pieces.
    let bishops = board.pieces[BISHOP];
    if s == 3 && w == 3 && board.piece_bb(strong, BISHOP) != 0 && board.piece_bb(weak, BISHOP) != 0
    {
        let light = 0x55AA_55AA_55AA_55AAu64;
        let sb = board.piece_bb(strong, BISHOP);
        let wb = bishops & !sb;
        if (sb & light != 0) != (wb & light != 0) {
            return 64;
        }
    }
    128
}

/// Static evaluation in centipawns from the side to move's perspective.
pub fn evaluate(board: &Board) -> i32 {
    let attacks = [all_attacks(board, WHITE), all_attacks(board, BLACK)];
    let s = evaluate_side(board, WHITE, &attacks) - evaluate_side(board, BLACK, &attacks);
    let phase = game_phase(board);
    let mut eg = s.1;
    let strong = if eg > 0 { WHITE } else { BLACK };
    eg = eg * scale_factor(board, strong) / 128;
    let score = (s.0 * phase + eg * (24 - phase)) / 24;
    let score = if board.stm == WHITE { score } else { -score };
    score + TEMPO
}

#[allow(dead_code)]
fn material_only(board: &Board) -> S {
    let mut s = S(0, 0);
    for p in PAWN..QUEEN + 1 {
        let diff = board.piece_bb(WHITE, p).count_ones() as i32
            - board.piece_bb(BLACK, p).count_ones() as i32;
        s += MATERIAL[p] * diff;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mirror(fen: &str) -> String {
        let b = Board::from_fen(fen).unwrap();
        let mut rows: Vec<String> = b
            .to_fen()
            .split(' ')
            .next()
            .unwrap()
            .split('/')
            .map(|r| {
                r.chars()
                    .map(|c| {
                        if c.is_ascii_uppercase() {
                            c.to_ascii_lowercase()
                        } else {
                            c.to_ascii_uppercase()
                        }
                    })
                    .collect()
            })
            .collect();
        rows.reverse();
        let stm = if b.stm == WHITE { "b" } else { "w" };
        format!("{} {stm} - - 0 1", rows.join("/"))
    }

    #[test]
    fn evaluation_is_color_symmetric() {
        for fen in [
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w - - 0 1",
            "r1bqkb1r/pppp1ppp/2n2n2/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w - - 4 4",
            "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R b - - 0 1",
        ] {
            let a = evaluate(&Board::from_fen(fen).unwrap());
            let b = evaluate(&Board::from_fen(&mirror(fen)).unwrap());
            assert_eq!(a, b, "{fen}");
        }
    }
}
