use crate::{
    evaluation::evaluate,
    game::Game,
    tt::{self, Bound, Entry, Table},
};
use chess::{BoardStatus, ChessMove, MoveGen, Piece};
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Instant;
const MATE: i32 = 30_000;

pub(crate) struct SearchControl {
    pub(crate) stop: Arc<AtomicBool>,
    pub(crate) deadline: Option<Instant>,
    pub(crate) node_limit: Option<u64>,
}

impl SearchControl {
    fn stopped(&self, nodes: u64) -> bool {
        self.stop.load(Ordering::Relaxed)
            || self.deadline.is_some_and(|d| Instant::now() >= d)
            || self.node_limit.is_some_and(|limit| nodes >= limit)
    }
}

pub(crate) const MAX_QPLY: i32 = 16;

pub(crate) fn quiescence(
    game: &mut Game,
    mut alpha: i32,
    beta: i32,
    ply: i32,
    qply: i32,
    control: &SearchControl,
    nodes: &mut u64,
) -> Option<i32> {
    if control.stopped(*nodes) {
        return None;
    }
    *nodes += 1;
    let board = game.board;
    match board.status() {
        BoardStatus::Checkmate => return Some(-MATE + ply),
        BoardStatus::Stalemate => return Some(0),
        BoardStatus::Ongoing => {}
    }
    if game.draw() {
        return Some(0);
    }
    let in_check = board.checkers().popcnt() > 0;
    if qply >= MAX_QPLY {
        // A static score while in check would illegally allow the side to pass.
        // Abort this iteration instead; iterative deepening retains its last result.
        return if in_check {
            None
        } else {
            Some(evaluate(&board))
        };
    }
    if !in_check {
        let stand_pat = evaluate(&board);
        if stand_pat >= beta {
            return Some(stand_pat);
        }
        alpha = alpha.max(stand_pat);
    }
    for mv in MoveGen::new_legal(&board) {
        let pawn = board.piece_on(mv.get_source()) == Some(Piece::Pawn);
        let capture = board.piece_on(mv.get_dest()).is_some()
            || (pawn && mv.get_source().get_file() != mv.get_dest().get_file());
        if !in_check && !capture && mv.get_promotion().is_none() {
            continue;
        }
        let halfmove = game.halfmove;
        game.play(mv);
        let result = quiescence(game, -beta, -alpha, ply + 1, qply + 1, control, nodes);
        game.unplay(board, halfmove);
        let score = -result?;
        if score >= beta {
            return Some(score);
        }
        alpha = alpha.max(score);
    }
    Some(alpha)
}

struct SearchState<'a> {
    hints: &'a mut HashMap<u64, ChessMove>,
    table: &'a mut Table,
    nodes: &'a mut u64,
}

fn search(
    game: &mut Game,
    depth: u8,
    mut alpha: i32,
    beta: i32,
    ply: i32,
    control: &SearchControl,
    state: &mut SearchState<'_>,
) -> Option<i32> {
    if control.stopped(*state.nodes) {
        return None;
    }
    *state.nodes += 1;
    let board = game.board;
    match board.status() {
        BoardStatus::Checkmate => return Some(-MATE + ply),
        BoardStatus::Stalemate => return Some(0),
        BoardStatus::Ongoing => {}
    }
    if game.draw() {
        return Some(0);
    }
    if depth == 0 {
        return quiescence(game, alpha, beta, ply, 0, control, state.nodes);
    }
    let original_alpha = alpha;
    let entry = state.table.get(board.get_hash());
    if game.halfmove == 0 {
        if let Some(entry) = entry.filter(|entry| entry.depth >= depth) {
            let score = tt::decode_mate(entry.score, ply);
            if entry.bound == Bound::Exact
                || (entry.bound == Bound::Lower && score >= beta)
                || (entry.bound == Bound::Upper && score <= alpha)
            {
                state.table.cutoffs += 1;
                return Some(score);
            }
        }
    }
    let mut moves: Vec<_> = MoveGen::new_legal(&board).collect();
    moves.sort_by_key(|mv| {
        std::cmp::Reverse((
            entry.and_then(|e| e.best) == Some(*mv)
                || state.hints.get(&board.get_hash()) == Some(mv),
            board.piece_on(mv.get_dest()).is_some(),
            mv.get_promotion().is_some(),
        ))
    });
    let mut best = None;
    let mut best_score = -MATE;
    for mv in moves {
        let halfmove = game.halfmove;
        game.play(mv);
        let result = search(game, depth - 1, -beta, -alpha, ply + 1, control, state);
        game.unplay(board, halfmove);
        let score = -result?;
        if best.is_none() || score > best_score {
            best = Some(mv);
            best_score = score;
        }
        if score > alpha {
            alpha = score;
            if state.hints.len() < 200_000 {
                state.hints.insert(board.get_hash(), mv);
            }
        }
        if alpha >= beta {
            break;
        }
    }
    if game.halfmove == 0 {
        let bound = if best_score <= original_alpha {
            Bound::Upper
        } else if best_score >= beta {
            Bound::Lower
        } else {
            Bound::Exact
        };
        state.table.put(
            board.get_hash(),
            Entry {
                depth,
                score: tt::encode_mate(best_score, ply),
                bound,
                best,
            },
        );
    }
    Some(alpha)
}

pub(crate) fn best_move(
    game: &mut Game,
    depth: u8,
    control: &SearchControl,
    hints: &mut HashMap<u64, ChessMove>,
    table: &mut Table,
    nodes: &mut u64,
) -> Option<(ChessMove, i32)> {
    let board = game.board;
    if game.draw() {
        return MoveGen::new_legal(&board).next().map(|mv| (mv, 0));
    }
    let mut best = None;
    let mut alpha = -MATE;
    let mut state = SearchState {
        hints,
        table,
        nodes,
    };
    for mv in MoveGen::new_legal(&board) {
        let halfmove = game.halfmove;
        game.play(mv);
        let result = search(
            game,
            depth.saturating_sub(1),
            -MATE,
            -alpha,
            1,
            control,
            &mut state,
        );
        game.unplay(board, halfmove);
        let score = -result?;
        if best.is_none() || score > alpha {
            best = Some(mv);
            alpha = score;
        }
    }
    best.map(|mv| (mv, alpha))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chess::Board;

    #[test]
    fn table_bounds_only_cut_off_valid_windows() {
        for (bound, score, expected_cutoff) in [
            (Bound::Exact, 37, true),
            (Bound::Lower, 100, true),
            (Bound::Lower, 0, false),
            (Bound::Upper, -100, true),
            (Bound::Upper, 0, false),
        ] {
            let mut game = Game::new(Board::default(), 0);
            let mut table = Table::new(64);
            table.put(
                game.board.get_hash(),
                Entry {
                    depth: 1,
                    score,
                    bound,
                    best: None,
                },
            );
            let mut hints = HashMap::new();
            let mut nodes = 0;
            let mut state = SearchState {
                hints: &mut hints,
                table: &mut table,
                nodes: &mut nodes,
            };
            let control = SearchControl {
                stop: Arc::new(AtomicBool::new(false)),
                deadline: None,
                node_limit: None,
            };
            assert!(search(&mut game, 1, -50, 50, 0, &control, &mut state).is_some());
            assert_eq!(
                state.table.cutoffs > 0,
                expected_cutoff,
                "bound {bound:?}, score {score}"
            );
        }
    }

    #[test]
    fn interrupted_search_restores_position_and_history() {
        let mut game = Game::new(Board::default(), 0);
        let original_board = game.board;
        let original_history = game.history.clone();
        let mut table = Table::new(64);
        let mut hints = HashMap::new();
        let mut nodes = 0;
        let mut state = SearchState {
            hints: &mut hints,
            table: &mut table,
            nodes: &mut nodes,
        };
        let control = SearchControl {
            stop: Arc::new(AtomicBool::new(false)),
            deadline: None,
            node_limit: Some(2),
        };
        assert!(search(&mut game, 4, -MATE, MATE, 0, &control, &mut state).is_none());
        assert_eq!(game.board, original_board);
        assert_eq!(game.history, original_history);
        assert_eq!(game.halfmove, 0);
    }
}
