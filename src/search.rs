use crate::{
    evaluation::evaluate,
    game::Game,
    tt::{self, Bound, Entry, Table},
};
use chess::{Board, BoardStatus, ChessMove, MoveGen, Piece};
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
        if !in_check {
            return Some(evaluate(&board));
        }
        // At the hard limit, examine every legal evasion once rather than
        // pretending that passing in check is legal or aborting the iteration.
        for mv in MoveGen::new_legal(&board) {
            if control.stopped(*nodes) {
                return None;
            }
            let halfmove = game.halfmove;
            game.play(mv);
            *nodes += 1;
            let score = match game.board.status() {
                BoardStatus::Checkmate => MATE - ply - 1,
                BoardStatus::Stalemate => 0,
                BoardStatus::Ongoing if game.draw() => 0,
                BoardStatus::Ongoing => -evaluate(&game.board),
            };
            game.unplay(board, halfmove);
            alpha = alpha.max(score);
            if alpha >= beta {
                break;
            }
        }
        return Some(alpha);
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
    killers: [[Option<ChessMove>; 2]; 128],
    history: [[i32; 64]; 64],
    pv: [[Option<ChessMove>; 64]; 65],
    pv_len: [usize; 65],
}

fn capture_value(board: &Board, mv: ChessMove) -> Option<i32> {
    let victim = board.piece_on(mv.get_dest()).or_else(|| {
        (board.piece_on(mv.get_source()) == Some(Piece::Pawn)
            && mv.get_source().get_file() != mv.get_dest().get_file())
        .then_some(Piece::Pawn)
    })?;
    let value = |piece| match piece {
        Piece::Pawn => 100,
        Piece::Knight => 320,
        Piece::Bishop => 330,
        Piece::Rook => 500,
        Piece::Queen => 900,
        Piece::King => 20_000,
    };
    Some(value(victim) * 10 - value(board.piece_on(mv.get_source()).unwrap()))
}

fn move_score(
    board: &Board,
    mv: ChessMove,
    hint: Option<ChessMove>,
    killers: [Option<ChessMove>; 2],
    history: &[[i32; 64]; 64],
) -> i32 {
    if Some(mv) == hint {
        return 1_000_000;
    }
    if let Some(value) = capture_value(board, mv) {
        return 100_000 + value;
    }
    if mv.get_promotion().is_some() {
        return 90_000;
    }
    if killers.contains(&Some(mv)) {
        return 80_000;
    }
    history[mv.get_source().to_index()][mv.get_dest().to_index()]
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
    let pv_index = (ply as usize).min(64);
    state.pv_len[pv_index] = 0;
    // Quiescence counts and validates its own entry node.
    if depth == 0 {
        return quiescence(game, alpha, beta, ply, 0, control, state.nodes);
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
    let hint = entry
        .and_then(|e| e.best)
        .or_else(|| state.hints.get(&board.get_hash()).copied());
    let mut best = None;
    let mut best_score = -MATE;
    for index in 0..moves.len() {
        let killers = state.killers[(ply as usize).min(127)];
        let best_index = (index..moves.len())
            .max_by_key(|&i| move_score(&board, moves[i], hint, killers, &state.history))
            .unwrap();
        moves.swap(index, best_index);
        let mv = moves[index];
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
            let child = (pv_index + 1).min(64);
            let count = state.pv_len[child].min(63);
            state.pv[pv_index][0] = Some(mv);
            for i in 0..count {
                state.pv[pv_index][i + 1] = state.pv[child][i];
            }
            state.pv_len[pv_index] = count + 1;
            if state.hints.len() < 200_000 {
                state.hints.insert(board.get_hash(), mv);
            }
        }
        if alpha >= beta {
            if capture_value(&board, mv).is_none() && mv.get_promotion().is_none() {
                let killers = &mut state.killers[(ply as usize).min(127)];
                if killers[0] != Some(mv) {
                    killers[1] = killers[0];
                    killers[0] = Some(mv);
                }
                let history =
                    &mut state.history[mv.get_source().to_index()][mv.get_dest().to_index()];
                *history = history.saturating_add(i32::from(depth).pow(2)).min(70_000);
            }
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
) -> Option<(ChessMove, i32, Vec<ChessMove>)> {
    let board = game.board;
    if game.draw() {
        return MoveGen::new_legal(&board)
            .next()
            .map(|mv| (mv, 0, vec![mv]));
    }
    let mut best = None;
    let mut alpha = -MATE;
    let mut state = SearchState {
        hints,
        table,
        nodes,
        killers: [[None; 2]; 128],
        history: [[0; 64]; 64],
        pv: [[None; 64]; 65],
        pv_len: [0; 65],
    };
    let mut best_pv = Vec::new();
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
            best_pv.clear();
            best_pv.push(mv);
            best_pv.extend(state.pv[1][..state.pv_len[1]].iter().flatten().copied());
        }
    }
    best.map(|mv| (mv, alpha, best_pv))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chess::Board;
    use std::str::FromStr;

    #[test]
    fn ordering_recognizes_en_passant_and_hash_move() {
        let game = crate::game::position("fen 4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1").unwrap();
        let capture = ChessMove::from_str("e5d6").unwrap();
        let quiet = ChessMove::from_str("e1d1").unwrap();
        let history = [[0; 64]; 64];
        assert!(capture_value(&game.board, capture).is_some());
        assert!(
            move_score(&game.board, capture, None, [None; 2], &history)
                > move_score(&game.board, quiet, None, [None; 2], &history)
        );
        assert!(
            move_score(&game.board, quiet, Some(quiet), [None; 2], &history)
                > move_score(&game.board, capture, None, [None; 2], &history)
        );
    }

    #[test]
    fn principal_variation_contains_legal_moves() {
        let mut game = Game::new(Board::default(), 0);
        let control = SearchControl {
            stop: Arc::new(AtomicBool::new(false)),
            deadline: None,
            node_limit: None,
        };
        let (best, _, pv) = best_move(
            &mut game,
            3,
            &control,
            &mut HashMap::new(),
            &mut Table::new(1024),
            &mut 0,
        )
        .unwrap();
        assert_eq!(pv.first(), Some(&best));
        assert!(pv.len() >= 2);
        for mv in pv {
            assert!(MoveGen::new_legal(&game.board).any(|legal| legal == mv));
            game.play(mv);
        }
    }

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
                killers: [[None; 2]; 128],
                history: [[0; 64]; 64],
                pv: [[None; 64]; 65],
                pv_len: [0; 65],
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
            killers: [[None; 2]; 128],
            history: [[0; 64]; 64],
            pv: [[None; 64]; 65],
            pv_len: [0; 65],
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

    #[test]
    fn leaf_counts_once_toward_node_limit() {
        let mut game = Game::new(Board::default(), 0);
        let mut table = Table::new(64);
        let mut hints = HashMap::new();
        let mut nodes = 0;
        let mut state = SearchState {
            hints: &mut hints,
            table: &mut table,
            nodes: &mut nodes,
            killers: [[None; 2]; 128],
            history: [[0; 64]; 64],
            pv: [[None; 64]; 65],
            pv_len: [0; 65],
        };
        let control = SearchControl {
            stop: Arc::new(AtomicBool::new(false)),
            deadline: None,
            node_limit: Some(1),
        };
        assert!(search(&mut game, 0, -MATE, MATE, 0, &control, &mut state).is_some());
        assert_eq!(*state.nodes, 1);
    }

    #[test]
    fn transposition_cache_preserves_search_scores() {
        for fen in [
            "startpos",
            "fen r1bq1rk1/ppp2ppp/2n2n2/3pp3/3PP3/2N2N2/PPP2PPP/R1BQ1RK1 w - - 0 6",
            "fen 8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        ] {
            let mut scores = Vec::new();
            for capacity in [0, 4096] {
                let mut game = crate::game::position(fen).unwrap();
                let mut table = Table::new(capacity);
                let mut hints = HashMap::new();
                let mut nodes = 0;
                let control = SearchControl {
                    stop: Arc::new(AtomicBool::new(false)),
                    deadline: None,
                    node_limit: None,
                };
                let depth = if fen == "startpos" { 4 } else { 3 };
                let score = best_move(
                    &mut game, depth, &control, &mut hints, &mut table, &mut nodes,
                )
                .unwrap()
                .1;
                if fen == "startpos" && capacity != 0 {
                    assert!(table.cutoffs > 0, "the test must exercise a cache cutoff");
                }
                scores.push(score);
            }
            assert_eq!(scores[0], scores[1], "{fen}");
        }
    }
}
