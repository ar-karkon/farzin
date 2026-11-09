use crate::{
    evaluation::{evaluate, evaluate_positional},
    game::Game,
    tt::{self, Bound, Entry, Table},
};
use chess::{BitBoard, Board, BoardStatus, ChessMove, MoveGen, Piece, Square};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use std::time::Instant;
const MATE: i32 = 30_000;

pub(crate) struct SearchControl {
    pub(crate) stop: Arc<AtomicBool>,
    pub(crate) deadline: Option<Instant>,
    pub(crate) node_limit: Option<u64>,
    pub(crate) shared_nodes: Option<Arc<AtomicU64>>,
    pub(crate) exchange_ordering: bool,
    pub(crate) pvs: bool,
    pub(crate) positional: bool,
    pub(crate) cache: RefCell<Option<farzin::caissa::CachedEvaluator>>,
    pub(crate) stack: RefCell<Option<farzin::caissa::SearchEvaluator>>,
}

impl SearchControl {
    fn push(&self, board: &Board) {
        if let Some(stack) = self.stack.borrow_mut().as_mut() {
            stack.push(board);
        }
    }
    fn pop(&self) {
        if let Some(stack) = self.stack.borrow_mut().as_mut() {
            stack.pop();
        }
    }
    fn evaluate<const STACK: bool>(&self, board: &Board) -> i32 {
        if STACK {
            if let Some(stack) = self.stack.borrow_mut().as_mut() {
                return stack.evaluate(board);
            }
        }
        self.cache.borrow_mut().as_mut().map_or_else(
            || {
                if self.positional {
                    evaluate_positional(board)
                } else {
                    evaluate(board)
                }
            },
            |cache| cache.evaluate(board),
        )
    }
    fn stopped(&self, nodes: u64) -> bool {
        self.stop.load(Ordering::Relaxed)
            || self.deadline.is_some_and(|d| Instant::now() >= d)
            || self.node_limit.is_some_and(|limit| {
                self.shared_nodes
                    .as_ref()
                    .map_or(nodes, |shared| shared.load(Ordering::Relaxed))
                    >= limit
            })
    }
    fn visit_node(&self, nodes: &mut u64) -> bool {
        if self.stopped(*nodes) {
            return false;
        }
        if let Some(shared) = &self.shared_nodes {
            if let Some(limit) = self.node_limit {
                let mut count = shared.load(Ordering::Relaxed);
                loop {
                    if count >= limit {
                        return false;
                    }
                    match shared.compare_exchange_weak(
                        count,
                        count + 1,
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                    ) {
                        Ok(_) => break,
                        Err(actual) => count = actual,
                    }
                }
            } else {
                shared.fetch_add(1, Ordering::Relaxed);
            }
        }
        *nodes += 1;
        true
    }
}

pub(crate) const MAX_QPLY: i32 = 16;

fn quiescence_impl<const STACK: bool>(
    game: &mut Game,
    mut alpha: i32,
    beta: i32,
    ply: i32,
    qply: i32,
    control: &SearchControl,
    nodes: &mut u64,
) -> Option<i32> {
    if !control.visit_node(nodes) {
        return None;
    }
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
            return Some(control.evaluate::<STACK>(&board));
        }
        // At the hard limit, examine every legal evasion once rather than
        // pretending that passing in check is legal or aborting the iteration.
        for mv in MoveGen::new_legal(&board) {
            if !control.visit_node(nodes) {
                return None;
            }
            let halfmove = game.halfmove;
            game.play(mv);
            if STACK {
                control.push(&game.board);
            }
            let score = match game.board.status() {
                BoardStatus::Checkmate => MATE - ply - 1,
                BoardStatus::Stalemate => 0,
                BoardStatus::Ongoing if game.draw() => 0,
                BoardStatus::Ongoing => -control.evaluate::<STACK>(&game.board),
            };
            if STACK {
                control.pop();
            }
            game.unplay(board, halfmove);
            alpha = alpha.max(score);
            if alpha >= beta {
                break;
            }
        }
        return Some(alpha);
    }
    if !in_check {
        let stand_pat = control.evaluate::<STACK>(&board);
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
        if STACK {
            control.push(&game.board);
        }
        let result =
            quiescence_impl::<STACK>(game, -beta, -alpha, ply + 1, qply + 1, control, nodes);
        if STACK {
            control.pop();
        }
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

fn piece_value(piece: Piece) -> i32 {
    match piece {
        Piece::Pawn => 100,
        Piece::Knight => 320,
        Piece::Bishop => 330,
        Piece::Rook => 500,
        Piece::Queen => 900,
        Piece::King => 20_000,
    }
}

// Local, bounded exchange minimax. Only legal recaptures on the target square
// are considered. It is an ordering heuristic, not a pruning or evaluation proof.
fn recapture_gain(board: &Board, target: Square, depth: u8) -> i32 {
    if depth == 0 {
        return 0;
    }
    let Some(victim) = board.piece_on(target) else {
        return 0;
    };
    let mut moves = MoveGen::new_legal(board);
    moves.set_iterator_mask(BitBoard::from_square(target));
    let mut best = 0;
    for mv in moves {
        let promotion = mv
            .get_promotion()
            .map_or(0, |p| piece_value(p) - piece_value(Piece::Pawn));
        let gain = piece_value(victim) + promotion
            - recapture_gain(&board.make_move_new(mv), target, depth - 1);
        best = best.max(gain);
    }
    best
}

fn exchange_value(board: &Board, mv: ChessMove) -> Option<i32> {
    capture_value(board, mv)?;
    let victim = board.piece_on(mv.get_dest()).unwrap_or(Piece::Pawn);
    let promotion = mv
        .get_promotion()
        .map_or(0, |p| piece_value(p) - piece_value(Piece::Pawn));
    Some(
        piece_value(victim) + promotion
            - recapture_gain(&board.make_move_new(mv), mv.get_dest(), 7),
    )
}

fn move_score(
    board: &Board,
    mv: ChessMove,
    hint: Option<ChessMove>,
    killers: [Option<ChessMove>; 2],
    history: &[[i32; 64]; 64],
    exchange_ordering: bool,
) -> i32 {
    if Some(mv) == hint {
        return 1_000_000;
    }
    if let Some(value) = capture_value(board, mv) {
        if !exchange_ordering {
            return 100_000 + value;
        }
        let value = exchange_value(board, mv).unwrap();
        return if value >= 0 {
            100_000 + value
        } else {
            -10_000 + value
        };
    }
    if mv.get_promotion().is_some() {
        return 90_000;
    }
    if killers.contains(&Some(mv)) {
        return 80_000;
    }
    history[mv.get_source().to_index()][mv.get_dest().to_index()]
}

fn search<const STACK: bool>(
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
        return quiescence_impl::<STACK>(game, alpha, beta, ply, 0, control, state.nodes);
    }
    if !control.visit_node(state.nodes) {
        return None;
    }
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
    // Legal chess positions have fewer than 256 legal moves. Keep the list on
    // the stack so every interior node does not allocate a new Vec.
    let mut generator = MoveGen::new_legal(&board);
    let first = generator
        .next()
        .expect("ongoing positions have legal moves");
    let mut moves = [first; 256];
    let mut move_count = 1;
    for mv in generator {
        assert!(
            move_count < moves.len(),
            "legal move list exceeded capacity"
        );
        moves[move_count] = mv;
        move_count += 1;
    }
    let hint = entry
        .and_then(|e| e.best)
        .or_else(|| state.hints.get(&board.get_hash()).copied());
    let killers = state.killers[(ply as usize).min(127)];
    let mut scores = [0; 256];
    for index in 0..move_count {
        scores[index] = move_score(
            &board,
            moves[index],
            hint,
            killers,
            &state.history,
            control.exchange_ordering,
        );
    }
    let mut best = None;
    let mut best_score = -MATE;
    for index in 0..move_count {
        let best_index = (index..move_count).max_by_key(|&i| scores[i]).unwrap();
        moves.swap(index, best_index);
        scores.swap(index, best_index);
        let mv = moves[index];
        let halfmove = game.halfmove;
        game.play(mv);
        if STACK {
            control.push(&game.board);
        }
        let result = if control.pvs && index > 0 && alpha + 1 < beta {
            match search::<STACK>(game, depth - 1, -alpha - 1, -alpha, ply + 1, control, state) {
                Some(probe) if -probe > alpha && -probe < beta => {
                    search::<STACK>(game, depth - 1, -beta, -alpha, ply + 1, control, state)
                }
                other => other,
            }
        } else {
            search::<STACK>(game, depth - 1, -beta, -alpha, ply + 1, control, state)
        };
        if STACK {
            control.pop();
        }
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
    best_move_window(game, depth, control, hints, table, nodes, (-MATE, MATE))
}

pub(crate) fn best_move_window(
    game: &mut Game,
    depth: u8,
    control: &SearchControl,
    hints: &mut HashMap<u64, ChessMove>,
    table: &mut Table,
    nodes: &mut u64,
    window: (i32, i32),
) -> Option<(ChessMove, i32, Vec<ChessMove>)> {
    let stack = control.stack.borrow().is_some();
    if stack {
        best_move_window_impl::<true>(game, depth, control, hints, table, nodes, window)
    } else {
        best_move_window_impl::<false>(game, depth, control, hints, table, nodes, window)
    }
}

#[cfg(test)]
pub(crate) fn quiescence(
    game: &mut Game,
    alpha: i32,
    beta: i32,
    ply: i32,
    qply: i32,
    control: &SearchControl,
    nodes: &mut u64,
) -> Option<i32> {
    let stack = control.stack.borrow().is_some();
    if stack {
        quiescence_impl::<true>(game, alpha, beta, ply, qply, control, nodes)
    } else {
        quiescence_impl::<false>(game, alpha, beta, ply, qply, control, nodes)
    }
}

fn best_move_window_impl<const STACK: bool>(
    game: &mut Game,
    depth: u8,
    control: &SearchControl,
    hints: &mut HashMap<u64, ChessMove>,
    table: &mut Table,
    nodes: &mut u64,
    window: (i32, i32),
) -> Option<(ChessMove, i32, Vec<ChessMove>)> {
    let board = game.board;
    if game.draw() {
        return MoveGen::new_legal(&board)
            .next()
            .map(|mv| (mv, 0, vec![mv]));
    }
    let mut best = None;
    let (mut alpha, beta) = window;
    let root_hint = hints
        .get(&board.get_hash())
        .copied()
        .filter(|hint| MoveGen::new_legal(&board).any(|mv| mv == *hint));
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
    let moves = root_hint
        .into_iter()
        .chain(MoveGen::new_legal(&board).filter(|mv| Some(*mv) != root_hint));
    for mv in moves {
        let halfmove = game.halfmove;
        game.play(mv);
        if STACK {
            control.push(&game.board);
        }
        let result = search::<STACK>(
            game,
            depth.saturating_sub(1),
            -beta,
            -alpha,
            1,
            control,
            &mut state,
        );
        if STACK {
            control.pop();
        }
        game.unplay(board, halfmove);
        let score = -result?;
        if best.is_none() || score > alpha {
            best = Some(mv);
            alpha = alpha.max(score);
            best_pv.clear();
            best_pv.push(mv);
            best_pv.extend(state.pv[1][..state.pv_len[1]].iter().flatten().copied());
        }
        if alpha >= beta {
            break;
        }
    }
    best.map(|mv| {
        if hints.len() < 200_000 {
            hints.insert(board.get_hash(), mv);
        }
        (mv, alpha, best_pv)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chess::Board;
    use std::str::FromStr;

    #[test]
    fn nnue_stack_search_cutoffs_and_interruption_restore_root() {
        let mut bytes = vec![0; 75_703_872];
        bytes[..4].copy_from_slice(&0x43534e4eu32.to_le_bytes());
        bytes[4..8].copy_from_slice(&15u32.to_le_bytes());
        for (i, byte) in bytes[64..].iter_mut().enumerate() {
            *byte = (i * 13 + 17) as u8;
        }
        let net = Arc::new(farzin::caissa::Network::from_bytes(bytes).unwrap());
        for limit in [None, Some(1), Some(25), Some(300)] {
            let mut signatures = Vec::new();
            for stack_enabled in [false, true] {
                let mut game = Game::new(Board::default(), 0);
                let original = game.history.clone();
                let control = SearchControl {
                    stop: Arc::new(AtomicBool::new(false)),
                    deadline: None,
                    node_limit: limit,
                    shared_nodes: None,
                    exchange_ordering: false,
                    pvs: true,
                    positional: false,
                    cache: RefCell::new((!stack_enabled).then(|| {
                        farzin::caissa::CachedEvaluator::new(Arc::clone(&net), &game.board)
                    })),
                    stack: RefCell::new(stack_enabled.then(|| {
                        farzin::caissa::SearchEvaluator::new(Arc::clone(&net), &game.board)
                    })),
                };
                let mut nodes = 0;
                let result = best_move(
                    &mut game,
                    3,
                    &control,
                    &mut HashMap::new(),
                    &mut Table::new(1024),
                    &mut nodes,
                );
                assert_eq!(game.board, Board::default());
                assert_eq!(game.history, original);
                assert_eq!(
                    control.evaluate::<true>(&game.board),
                    net.evaluate(&game.board)
                );
                signatures.push((result, nodes));
            }
            assert_eq!(signatures[0], signatures[1]);
        }
    }

    #[test]
    fn ordering_recognizes_en_passant_and_hash_move() {
        let game = crate::game::position("fen 4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1").unwrap();
        let capture = ChessMove::from_str("e5d6").unwrap();
        let quiet = ChessMove::from_str("e1d1").unwrap();
        let history = [[0; 64]; 64];
        assert!(capture_value(&game.board, capture).is_some());
        assert!(
            move_score(&game.board, capture, None, [None; 2], &history, false)
                > move_score(&game.board, quiet, None, [None; 2], &history, false)
        );
        assert!(
            move_score(&game.board, quiet, Some(quiet), [None; 2], &history, false)
                > move_score(&game.board, capture, None, [None; 2], &history, false)
        );
    }

    #[test]
    fn exchange_ordering_accounts_for_legal_recaptures() {
        let game = crate::game::position("fen 3q3k/8/8/8/8/8/3r4/K2Q4 w - - 0 1").unwrap();
        let capture = ChessMove::from_str("d1d2").unwrap();
        assert_eq!(exchange_value(&game.board, capture), Some(-400));
        let game = crate::game::position("fen 7k/8/8/3pP3/8/8/8/K7 w - d6 0 1").unwrap();
        assert_eq!(
            exchange_value(&game.board, ChessMove::from_str("e5d6").unwrap()),
            Some(100)
        );
    }

    #[test]
    fn experimental_exchange_ordering_preserves_score() {
        let fen = "fen 3q3k/8/8/8/8/8/3r4/K2Q4 w - - 0 1";
        let mut scores = Vec::new();
        for enabled in [false, true] {
            let mut game = crate::game::position(fen).unwrap();
            let control = SearchControl {
                stop: Arc::new(AtomicBool::new(false)),
                deadline: None,
                node_limit: None,
                shared_nodes: None,
                exchange_ordering: enabled,
                pvs: false,
                positional: false,
                cache: RefCell::new(None),
                stack: RefCell::new(None),
            };
            scores.push(
                best_move(
                    &mut game,
                    3,
                    &control,
                    &mut HashMap::new(),
                    &mut Table::new(1024),
                    &mut 0,
                )
                .unwrap()
                .1,
            );
        }
        assert_eq!(scores[0], scores[1]);
    }

    #[test]
    fn pvs_matches_alpha_beta_on_reference_positions() {
        for fen in ["startpos", "fen 8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1"] {
            let mut scores = Vec::new();
            for pvs in [false, true] {
                let mut game = crate::game::position(fen).unwrap();
                let control = SearchControl {
                    stop: Arc::new(AtomicBool::new(false)),
                    deadline: None,
                    node_limit: None,
                    shared_nodes: None,
                    exchange_ordering: false,
                    pvs,
                    positional: false,
                    cache: RefCell::new(None),
                    stack: RefCell::new(None),
                };
                scores.push(
                    best_move(
                        &mut game,
                        4,
                        &control,
                        &mut HashMap::new(),
                        &mut Table::new(4096),
                        &mut 0,
                    )
                    .unwrap()
                    .1,
                );
            }
            assert_eq!(scores[0], scores[1], "{fen}");
        }
    }

    #[test]
    fn principal_variation_contains_legal_moves() {
        let mut game = Game::new(Board::default(), 0);
        let control = SearchControl {
            stop: Arc::new(AtomicBool::new(false)),
            deadline: None,
            node_limit: None,
            shared_nodes: None,
            exchange_ordering: false,
            pvs: false,
            positional: false,
            cache: RefCell::new(None),
            stack: RefCell::new(None),
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
    fn root_search_prefers_valid_hint_when_scores_tie() {
        let mut game = Game::new(Board::default(), 0);
        let control = SearchControl {
            stop: Arc::new(AtomicBool::new(false)),
            deadline: None,
            node_limit: None,
            shared_nodes: None,
            exchange_ordering: false,
            pvs: false,
            positional: false,
            cache: RefCell::new(None),
            stack: RefCell::new(None),
        };
        let hint = ChessMove::from_str("h2h4").unwrap();
        let mut hints = HashMap::new();
        hints.insert(game.board.get_hash(), hint);
        let result = best_move(
            &mut game,
            1,
            &control,
            &mut hints,
            &mut Table::new(0),
            &mut 0,
        )
        .unwrap();
        assert_eq!(result.0, hint);
        assert_eq!(hints.get(&game.board.get_hash()), Some(&hint));
        // A stale or illegal hint must never become a best move.
        hints.insert(game.board.get_hash(), ChessMove::from_str("e2e5").unwrap());
        let result = best_move(
            &mut game,
            1,
            &control,
            &mut hints,
            &mut Table::new(0),
            &mut 0,
        )
        .unwrap();
        assert!(MoveGen::new_legal(&game.board).any(|mv| mv == result.0));
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
                shared_nodes: None,
                exchange_ordering: false,
                pvs: false,
                positional: false,
                cache: RefCell::new(None),
                stack: RefCell::new(None),
            };
            assert!(search::<false>(&mut game, 1, -50, 50, 0, &control, &mut state).is_some());
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
            shared_nodes: None,
            exchange_ordering: false,
            pvs: false,
            positional: false,
            cache: RefCell::new(None),
            stack: RefCell::new(None),
        };
        assert!(search::<false>(&mut game, 4, -MATE, MATE, 0, &control, &mut state).is_none());
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
            shared_nodes: None,
            exchange_ordering: false,
            pvs: false,
            positional: false,
            cache: RefCell::new(None),
            stack: RefCell::new(None),
        };
        assert!(search::<false>(&mut game, 0, -MATE, MATE, 0, &control, &mut state).is_some());
        assert_eq!(*state.nodes, 1);
    }

    #[test]
    fn shared_node_limit_is_exact_under_contention() {
        let shared = Arc::new(AtomicU64::new(0));
        let mut workers = Vec::new();
        for _ in 0..4 {
            let counter = Arc::clone(&shared);
            workers.push(std::thread::spawn(move || {
                let control = SearchControl {
                    stop: Arc::new(AtomicBool::new(false)),
                    deadline: None,
                    node_limit: Some(1000),
                    shared_nodes: Some(counter),
                    exchange_ordering: false,
                    pvs: false,
                    positional: false,
                    cache: RefCell::new(None),
                    stack: RefCell::new(None),
                };
                let mut nodes = 0;
                while control.visit_node(&mut nodes) {}
                nodes
            }));
        }
        let total: u64 = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .sum();
        assert_eq!(total, 1000);
        assert_eq!(shared.load(Ordering::Relaxed), 1000);
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
                    shared_nodes: None,
                    exchange_ordering: false,
                    pvs: false,
                    positional: false,
                    cache: RefCell::new(None),
                    stack: RefCell::new(None),
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

    #[test]
    fn transposition_score_is_not_used_with_nonzero_halfmove_clock() {
        let mut game = Game::new(Board::default(), 99);
        let mut table = Table::new(64);
        table.put(
            game.board.get_hash(),
            Entry {
                depth: 10,
                score: 12_345,
                bound: Bound::Exact,
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
            shared_nodes: None,
            exchange_ordering: false,
            pvs: false,
            positional: false,
            cache: RefCell::new(None),
            stack: RefCell::new(None),
        };
        let score = search::<false>(&mut game, 1, -MATE, MATE, 0, &control, &mut state).unwrap();
        assert_ne!(score, 12_345);
        assert_eq!(state.table.cutoffs, 0);
    }

    #[test]
    fn pvs_and_cache_match_reference_along_legal_game() {
        let mut game = Game::new(Board::default(), 0);
        for ply in 0..12 {
            let legal: Vec<_> = MoveGen::new_legal(&game.board).collect();
            if legal.is_empty() || game.draw() {
                break;
            }
            let mut scores = Vec::new();
            for (pvs, capacity) in [(false, 0), (false, 4096), (true, 4096)] {
                let control = SearchControl {
                    stop: Arc::new(AtomicBool::new(false)),
                    deadline: None,
                    node_limit: None,
                    shared_nodes: None,
                    exchange_ordering: false,
                    pvs,
                    positional: false,
                    cache: RefCell::new(None),
                    stack: RefCell::new(None),
                };
                let mut position = game.clone();
                scores.push(
                    best_move(
                        &mut position,
                        3,
                        &control,
                        &mut HashMap::new(),
                        &mut Table::new(capacity),
                        &mut 0,
                    )
                    .unwrap()
                    .1,
                );
            }
            assert_eq!(scores[0], scores[1], "cache at ply {ply}");
            assert_eq!(scores[0], scores[2], "pvs at ply {ply}");
            game.play(legal[(ply * 17 + 3) % legal.len()]);
        }
    }

    #[test]
    fn aspiration_failures_research_to_full_window_score() {
        for pvs in [false, true] {
            for window in [(-200, -100), (100, 200)] {
                let mut game = Game::new(Board::default(), 0);
                let control = SearchControl {
                    stop: Arc::new(AtomicBool::new(false)),
                    deadline: None,
                    node_limit: None,
                    shared_nodes: None,
                    exchange_ordering: false,
                    pvs,
                    positional: false,
                    cache: RefCell::new(None),
                    stack: RefCell::new(None),
                };
                let mut table = Table::new(4096);
                let mut hints = HashMap::new();
                let mut nodes = 0;
                let trial = best_move_window(
                    &mut game, 3, &control, &mut hints, &mut table, &mut nodes, window,
                )
                .unwrap();
                assert!(
                    trial.1 <= window.0 || trial.1 >= window.1,
                    "expected window failure: {window:?} score {}",
                    trial.1
                );
                let researched =
                    best_move(&mut game, 3, &control, &mut hints, &mut table, &mut nodes)
                        .unwrap()
                        .1;
                let reference = best_move(
                    &mut game,
                    3,
                    &control,
                    &mut HashMap::new(),
                    &mut Table::new(0),
                    &mut 0,
                )
                .unwrap()
                .1;
                assert_eq!(researched, reference, "window {window:?}, pvs {pvs}");
            }
        }
    }

    #[test]
    fn interrupted_aspiration_preserves_game() {
        let mut game = Game::new(Board::default(), 0);
        let board = game.board;
        let history = game.history.clone();
        let control = SearchControl {
            stop: Arc::new(AtomicBool::new(false)),
            deadline: None,
            node_limit: Some(2),
            shared_nodes: None,
            exchange_ordering: false,
            pvs: true,
            positional: false,
            cache: RefCell::new(None),
            stack: RefCell::new(None),
        };
        assert!(best_move_window(
            &mut game,
            4,
            &control,
            &mut HashMap::new(),
            &mut Table::new(128),
            &mut 0,
            (-50, 50)
        )
        .is_none());
        assert_eq!(game.board, board);
        assert_eq!(game.history, history);
        assert_eq!(game.halfmove, 0);
    }
}
