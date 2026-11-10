//! Pseudo-legal move generation. Legality is checked with `Board::is_legal`.
use crate::attacks::*;
use crate::board::Board;
use crate::types::*;

pub const MAX_MOVES: usize = 256;

pub struct MoveList {
    pub moves: [Move; MAX_MOVES],
    pub scores: [i32; MAX_MOVES],
    pub len: usize,
}

impl Default for MoveList {
    fn default() -> Self {
        Self::new()
    }
}

impl MoveList {
    #[inline]
    pub fn new() -> Self {
        MoveList {
            moves: [Move::NONE; MAX_MOVES],
            scores: [0; MAX_MOVES],
            len: 0,
        }
    }
    #[inline(always)]
    pub fn push(&mut self, mv: Move) {
        debug_assert!(self.len < MAX_MOVES);
        self.moves[self.len] = mv;
        self.len += 1;
    }
    pub fn iter(&self) -> impl Iterator<Item = Move> + '_ {
        self.moves[..self.len].iter().copied()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Noisy,
    Quiet,
    All,
}

#[inline(always)]
fn push_promotions(list: &mut MoveList, from: Square, to: Square, capture: bool, kind: Kind) {
    let base = if capture { PROMO_CAPTURE } else { PROMO };
    // Queen promotions are noisy; underpromotions go with the quiets unless
    // they capture (they are rarely good, but quiescence still sees captures).
    if kind != Kind::Quiet {
        list.push(Move::new(from, to, base | 3));
    }
    if kind == Kind::All || (kind == Kind::Quiet && !capture) || (kind == Kind::Noisy && capture) {
        for p in 0..3 {
            list.push(Move::new(from, to, base | p));
        }
    }
}

fn generate(board: &Board, list: &mut MoveList, kind: Kind) {
    let us = board.stm;
    let them = us ^ 1;
    let occ = board.occupied();
    let own = board.colors[us];
    let enemy = board.colors[them];
    let king = board.king_sq(us);

    // King moves.
    let king_targets = match kind {
        Kind::Noisy => enemy,
        Kind::Quiet => !occ,
        Kind::All => !own,
    };
    let mut targets = KING_ATTACKS[king] & king_targets;
    while targets != 0 {
        let to = pop_lsb(&mut targets);
        let flag = if enemy & bit(to) != 0 { CAPTURE } else { QUIET };
        list.push(Move::new(king, to, flag));
    }
    if board.checkers.count_ones() > 1 {
        return;
    }
    // Squares that resolve a single check (or everything when not in check).
    let evasion = if board.checkers != 0 {
        let checker = lsb(board.checkers);
        board.checkers | BETWEEN[king][checker]
    } else {
        !0
    };
    if kind != Kind::Noisy && board.checkers == 0 {
        for flag in [KING_CASTLE, QUEEN_CASTLE] {
            if board.castle_ok(flag) {
                let to = if flag == KING_CASTLE {
                    king + 2
                } else {
                    king - 2
                };
                list.push(Move::new(king, to, flag));
            }
        }
    }

    let piece_targets = evasion
        & match kind {
            Kind::Noisy => enemy,
            Kind::Quiet => !occ,
            Kind::All => !own,
        };
    for piece in [KNIGHT, BISHOP, ROOK, QUEEN] {
        let mut froms = board.piece_bb(us, piece);
        while froms != 0 {
            let from = pop_lsb(&mut froms);
            let mut targets = piece_attacks(piece, from, occ) & piece_targets;
            while targets != 0 {
                let to = pop_lsb(&mut targets);
                let flag = if enemy & bit(to) != 0 { CAPTURE } else { QUIET };
                list.push(Move::new(from, to, flag));
            }
        }
    }

    // Pawns.
    let pawns = board.piece_bb(us, PAWN);
    let (up, promo_rank, third_rank): (i32, Bitboard, Bitboard) = if us == WHITE {
        (8, RANK_8, RANK_1 << 16)
    } else {
        (-8, RANK_1, RANK_1 << 40)
    };
    let shift = |bb: Bitboard, d: i32| -> Bitboard {
        if d > 0 {
            bb << d
        } else {
            bb >> -d
        }
    };
    let empty = !occ;
    let single = shift(pawns, up) & empty;
    let double = shift(single & third_rank, up) & empty & evasion;
    let single = single & evasion;

    // Pushes.
    let mut promo_push = single & promo_rank;
    while promo_push != 0 {
        let to = pop_lsb(&mut promo_push);
        push_promotions(list, (to as i32 - up) as usize, to, false, kind);
    }
    if kind != Kind::Noisy {
        let mut quiet = single & !promo_rank;
        while quiet != 0 {
            let to = pop_lsb(&mut quiet);
            list.push(Move::new((to as i32 - up) as usize, to, QUIET));
        }
        let mut dbl = double;
        while dbl != 0 {
            let to = pop_lsb(&mut dbl);
            list.push(Move::new((to as i32 - 2 * up) as usize, to, DOUBLE_PUSH));
        }
    }

    // Captures (including capture-promotions and en passant).
    if kind != Kind::Quiet {
        let mut froms = pawns;
        while froms != 0 {
            let from = pop_lsb(&mut froms);
            let attacks = PAWN_ATTACKS[us][from];
            let mut caps = attacks & enemy & evasion;
            while caps != 0 {
                let to = pop_lsb(&mut caps);
                if bit(to) & promo_rank != 0 {
                    push_promotions(list, from, to, true, kind);
                } else if kind != Kind::Quiet {
                    list.push(Move::new(from, to, CAPTURE));
                }
            }
            if kind != Kind::Quiet && board.ep != 64 && attacks & bit(board.ep as usize) != 0 {
                // In check, en passant only helps if it captures the checker
                // or blocks; is_legal settles the rest.
                let ep = board.ep as usize;
                if board.checkers == 0 || evasion & (bit(ep) | bit(ep ^ 8)) != 0 {
                    list.push(Move::new(from, ep, EN_PASSANT));
                }
            }
        }
    }
}

pub fn generate_all(board: &Board, list: &mut MoveList) {
    generate(board, list, Kind::All);
}

/// Captures and queen promotions (plus capture underpromotions).
pub fn generate_noisy(board: &Board, list: &mut MoveList) {
    generate(board, list, Kind::Noisy);
}

/// Non-captures, including quiet underpromotions.
pub fn generate_quiets(board: &Board, list: &mut MoveList) {
    generate(board, list, Kind::Quiet);
}

pub fn legal_moves(board: &Board) -> Vec<Move> {
    let mut list = MoveList::new();
    generate_all(board, &mut list);
    list.iter().filter(|m| board.is_legal(*m)).collect()
}

pub fn perft(board: &Board, depth: u32) -> u64 {
    let mut list = MoveList::new();
    generate_all(board, &mut list);
    let mut nodes = 0;
    for mv in list.iter() {
        if !board.is_legal(mv) {
            continue;
        }
        if depth <= 1 {
            nodes += 1;
        } else {
            nodes += perft(&board.make_move(mv), depth - 1);
        }
    }
    nodes
}
