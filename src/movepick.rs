//! Staged move picker: TT move, good noisy, killers, counter, quiets, bad noisy.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::movegen::legal_moves;

    #[test]
    fn quiet_ordering_prioritizes_an_attacked_queen() {
        let board = Board::from_fen("3r4/7k/8/8/3Q4/8/8/K6N w - - 0 1").unwrap();
        let histories = Histories::new();
        let mut picker = MovePicker::new(
            &board,
            Move::NONE,
            [Move::NONE; 2],
            Move::NONE,
            OrderingContext { cont: [None; 3] },
        );
        let first_quiet = std::iter::from_fn(|| picker.next(&board, &histories))
            .find(|m| !m.is_noisy() && board.is_legal(*m))
            .unwrap();
        assert_eq!(first_quiet.from(), 27); // d4
    }

    #[test]
    fn staged_ordering_preserves_every_legal_move_once() {
        for fen in [
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1",
            "1r2k3/P7/8/8/8/8/7p/4K3 w - - 0 1",
        ] {
            let mut board = Board::from_fen(fen).unwrap();
            let histories = Histories::new();
            let mut seed = 17usize;
            for _ in 0..48 {
                let legal = legal_moves(&board);
                if legal.is_empty() {
                    break;
                }
                let quiets: Vec<_> = legal.iter().copied().filter(|m| !m.is_noisy()).collect();
                let k1 = quiets.first().copied().unwrap_or(Move::NONE);
                let k2 = quiets.get(1).copied().unwrap_or(Move::NONE);
                let mut picker = MovePicker::new(
                    &board,
                    legal[0],
                    [k1, k2],
                    k1,
                    OrderingContext { cont: [None; 3] },
                );
                let mut actual = Vec::new();
                while let Some(mv) = picker.next(&board, &histories) {
                    if board.is_legal(mv) {
                        actual.push(mv.0);
                    }
                    assert!(actual.len() <= 256);
                }
                actual.sort_unstable();
                let mut expected: Vec<_> = legal.iter().map(|m| m.0).collect();
                expected.sort_unstable();
                assert_eq!(actual, expected, "{}", board.to_fen());
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                board = board.make_move(legal[(seed >> 33) % legal.len()]);
            }
        }
    }
}

/// Context needed to score quiet moves with continuation history.
#[derive(Clone, Copy)]
pub struct OrderingContext {
    /// `piece_code * 64 + to` of the moves 1, 2 and 4 plies ago, if any.
    pub cont: [Option<usize>; 3],
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
        // Prefer saving pieces attacked by cheaper enemy pieces, and delay
        // quiet moves onto those attacks. Compute the masks once per quiet
        // stage rather than running SEE for every ordering comparison.
        let enemy = board.colors[stm ^ 1];
        let mut pawns = enemy & board.pieces[PAWN];
        let mut pawn_attacks = 0;
        while pawns != 0 {
            pawn_attacks |= crate::attacks::PAWN_ATTACKS[stm ^ 1][pop_lsb(&mut pawns)];
        }
        let mut minor_attacks = pawn_attacks;
        let mut knights = enemy & board.pieces[KNIGHT];
        while knights != 0 {
            minor_attacks |= crate::attacks::KNIGHT_ATTACKS[pop_lsb(&mut knights)];
        }
        let occ = board.occupied();
        let mut bishops = enemy & board.pieces[BISHOP];
        while bishops != 0 {
            minor_attacks |= crate::attacks::bishop_attacks(pop_lsb(&mut bishops), occ);
        }
        let mut rook_attacks = minor_attacks;
        let mut rooks = enemy & board.pieces[ROOK];
        while rooks != 0 {
            rook_attacks |= crate::attacks::rook_attacks(pop_lsb(&mut rooks), occ);
        }
        let threats = [
            0,
            pawn_attacks,
            pawn_attacks,
            minor_attacks,
            rook_attacks,
            0,
        ];
        for i in 0..self.list.len {
            let mv = self.list.moves[i];
            let piece = board.piece_on(mv.from()).unwrap();
            let mask = threats[piece];
            let escape =
                i32::from(mask & bit(mv.from()) != 0) - i32::from(mask & bit(mv.to()) != 0);
            self.list.scores[i] =
                h.quiet_score(board, stm, mv, &self.ctx) + escape * 8 * SEE_VALUES[piece];
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
