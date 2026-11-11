//! Staged move picker: TT move, good noisy, killers, counter, quiets, bad noisy.
use crate::attacks::{bishop_attacks, rook_attacks, KNIGHT_ATTACKS, PAWN_ATTACKS};
use crate::board::{Board, SEE_VALUES};
use crate::movegen::{generate_all, generate_noisy, generate_quiets, MoveList};
use crate::search::Histories;
use crate::types::*;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Stage {
    TtMove,
    GenNoisy,
    GoodNoisy,
    Killer1,
    Killer2,
    Counter,
    GenQuiets,
    Quiets,
    BadNoisy,
    // Quiescence / evasion stages.
    QsTtMove,
    QsGen,
    QsMoves,
    Done,
}

/// Context needed to score quiet moves with continuation history.
#[derive(Clone, Copy)]
pub struct OrderingContext {
    /// `piece_code * 64 + to` of the moves 1, 2 and 4 plies ago, if any.
    pub cont: [Option<usize>; 3],
    /// Pawn-structure history bucket, if pawn history is enabled.
    pub pawn: Option<usize>,
    /// Threat-aware quiet ordering (escape bonus, en-prise malus, checks).
    pub threats: bool,
}

pub struct MovePicker {
    stage: Stage,
    tt_move: Move,
    killers: [Move; 2],
    counter: Move,
    list: MoveList,
    idx: usize,
    bad: MoveList,
    bad_idx: usize,
    pub skip_quiets: bool,
    ctx: OrderingContext,
    evasions: bool,
}

#[inline]
fn victim_value(board: &Board, mv: Move) -> i32 {
    if mv.flag() == EN_PASSANT {
        SEE_VALUES[PAWN]
    } else {
        board.piece_on(mv.to()).map_or(0, |p| SEE_VALUES[p])
    }
}

impl MovePicker {
    pub fn new(
        board: &Board,
        tt_move: Move,
        killers: [Move; 2],
        counter: Move,
        ctx: OrderingContext,
    ) -> Self {
        let tt_ok = board.is_pseudo_legal(tt_move);
        MovePicker {
            stage: if tt_ok {
                Stage::TtMove
            } else {
                Stage::GenNoisy
            },
            tt_move: if tt_ok { tt_move } else { Move::NONE },
            killers,
            counter,
            list: MoveList::new(),
            idx: 0,
            bad: MoveList::new(),
            bad_idx: 0,
            skip_quiets: false,
            ctx,
            evasions: false,
        }
    }

    /// Quiescence picker: noisy moves only, or all evasions when in check.
    pub fn new_qsearch(board: &Board, tt_move: Move, ctx: OrderingContext) -> Self {
        let in_check = board.in_check();
        let tt_ok = board.is_pseudo_legal(tt_move) && (in_check || tt_move.is_noisy());
        MovePicker {
            stage: if tt_ok { Stage::QsTtMove } else { Stage::QsGen },
            tt_move: if tt_ok { tt_move } else { Move::NONE },
            killers: [Move::NONE; 2],
            counter: Move::NONE,
            list: MoveList::new(),
            idx: 0,
            bad: MoveList::new(),
            bad_idx: 0,
            skip_quiets: false,
            ctx,
            evasions: in_check,
        }
    }

    fn score_noisy(&mut self, board: &Board, h: &Histories) {
        for i in 0..self.list.len {
            let mv = self.list.moves[i];
            let piece = board.mailbox[mv.from()] as usize;
            let victim = if mv.flag() == EN_PASSANT {
                PAWN
            } else {
                board.piece_on(mv.to()).unwrap_or(PAWN)
            };
            let mut score =
                victim_value(board, mv) * 16 + i32::from(h.capture[piece][mv.to()][victim]);
            if mv.is_promotion() && mv.promotion() == QUEEN {
                score += 16 * 800;
            }
            self.list.scores[i] = score;
        }
    }

    fn score_quiets(&mut self, board: &Board, h: &Histories) {
        let stm = board.stm;
        if !self.ctx.threats {
            for i in 0..self.list.len {
                let mv = self.list.moves[i];
                self.list.scores[i] = h.quiet_score(board, stm, mv, &self.ctx);
            }
            return;
        }
        let them = stm ^ 1;
        let (by_pawn, by_minor, by_rook) = board.threats(them);
        let occ = board.occupied();
        let ksq = board.king_sq(them);
        let bishop = bishop_attacks(ksq, occ);
        let rook = rook_attacks(ksq, occ);
        // Squares from which a piece of each type would give check.
        let check_sq = [
            PAWN_ATTACKS[them][ksq],
            KNIGHT_ATTACKS[ksq],
            bishop,
            rook,
            bishop | rook,
            0,
        ];
        for i in 0..self.list.len {
            let mv = self.list.moves[i];
            let pt = board.mailbox[mv.from()] as usize % 6;
            let from = bit(mv.from());
            let to = bit(mv.to());
            let mut score = h.quiet_score(board, stm, mv, &self.ctx);
            if check_sq[pt] & to != 0 {
                score += 8192;
            }
            let (threat, escape, malus) = match pt {
                QUEEN => (by_rook, 25_000, 24_000),
                ROOK => (by_minor, 12_500, 12_000),
                KNIGHT | BISHOP => (by_pawn, 7_000, 4_000),
                _ => (0, 0, 0),
            };
            if threat & from != 0 && threat & to == 0 {
                score += escape;
            } else if threat & to != 0 {
                score -= malus;
            }
            self.list.scores[i] = score;
        }
    }

    fn score_evasions(&mut self, board: &Board, h: &Histories) {
        let stm = board.stm;
        for i in 0..self.list.len {
            let mv = self.list.moves[i];
            self.list.scores[i] = if mv.is_noisy() {
                1 << 24 | (victim_value(board, mv) * 16 - board.piece_on(mv.from()).unwrap() as i32)
            } else {
                h.quiet_score(board, stm, mv, &self.ctx)
            };
        }
    }

    /// Pick the best remaining move in `list[idx..]` (selection sort step).
    #[inline]
    fn pick_best(&mut self) -> Option<Move> {
        if self.idx >= self.list.len {
            return None;
        }
        let mut best = self.idx;
        for i in self.idx + 1..self.list.len {
            if self.list.scores[i] > self.list.scores[best] {
                best = i;
            }
        }
        self.list.moves.swap(self.idx, best);
        self.list.scores.swap(self.idx, best);
        let mv = self.list.moves[self.idx];
        self.idx += 1;
        Some(mv)
    }

    fn is_special(&self, mv: Move) -> bool {
        mv == self.tt_move || mv == self.killers[0] || mv == self.killers[1] || mv == self.counter
    }

    pub fn next(&mut self, board: &Board, h: &Histories) -> Option<Move> {
        loop {
            match self.stage {
                Stage::TtMove => {
                    self.stage = Stage::GenNoisy;
                    return Some(self.tt_move);
                }
                Stage::GenNoisy => {
                    self.list.len = 0;
                    generate_noisy(board, &mut self.list);
                    self.score_noisy(board, h);
                    self.idx = 0;
                    self.stage = Stage::GoodNoisy;
                }
                Stage::GoodNoisy => match self.pick_best() {
                    Some(mv) => {
                        if mv == self.tt_move {
                            continue;
                        }
                        let score = self.list.scores[self.idx - 1];
                        if !board.see_ge(mv, -score / 32) {
                            self.bad.push(mv);
                            continue;
                        }
                        return Some(mv);
                    }
                    None => self.stage = Stage::Killer1,
                },
                Stage::Killer1 => {
                    self.stage = Stage::Killer2;
                    let k = self.killers[0];
                    if !self.skip_quiets
                        && k != self.tt_move
                        && !k.is_noisy()
                        && board.is_pseudo_legal(k)
                    {
                        return Some(k);
                    }
                }
                Stage::Killer2 => {
                    self.stage = Stage::Counter;
                    let k = self.killers[1];
                    if !self.skip_quiets
                        && k != self.tt_move
                        && k != self.killers[0]
                        && !k.is_noisy()
                        && board.is_pseudo_legal(k)
                    {
                        return Some(k);
                    }
                }
                Stage::Counter => {
                    self.stage = Stage::GenQuiets;
                    let c = self.counter;
                    if !self.skip_quiets
                        && c != self.tt_move
                        && c != self.killers[0]
                        && c != self.killers[1]
                        && !c.is_noisy()
                        && board.is_pseudo_legal(c)
                    {
                        return Some(c);
                    }
                }
                Stage::GenQuiets => {
                    self.stage = Stage::Quiets;
                    if !self.skip_quiets {
                        self.list.len = 0;
                        generate_quiets(board, &mut self.list);
                        self.score_quiets(board, h);
                        self.idx = 0;
                    } else {
                        self.list.len = 0;
                        self.idx = 0;
                    }
                }
                Stage::Quiets => {
                    if self.skip_quiets {
                        self.stage = Stage::BadNoisy;
                        continue;
                    }
                    match self.pick_best() {
                        Some(mv) => {
                            if self.is_special(mv) {
                                continue;
                            }
                            return Some(mv);
                        }
                        None => self.stage = Stage::BadNoisy,
                    }
                }
                Stage::BadNoisy => {
                    if self.bad_idx < self.bad.len {
                        let mv = self.bad.moves[self.bad_idx];
                        self.bad_idx += 1;
                        return Some(mv);
                    }
                    self.stage = Stage::Done;
                }
                Stage::QsTtMove => {
                    self.stage = Stage::QsGen;
                    return Some(self.tt_move);
                }
                Stage::QsGen => {
                    self.list.len = 0;
                    if self.evasions {
                        generate_all(board, &mut self.list);
                        self.score_evasions(board, h);
                    } else {
                        generate_noisy(board, &mut self.list);
                        self.score_noisy(board, h);
                    }
                    self.idx = 0;
                    self.stage = Stage::QsMoves;
                }
                Stage::QsMoves => match self.pick_best() {
                    Some(mv) => {
                        if mv == self.tt_move {
                            continue;
                        }
                        return Some(mv);
                    }
                    None => self.stage = Stage::Done,
                },
                Stage::Done => return None,
            }
        }
    }
}
