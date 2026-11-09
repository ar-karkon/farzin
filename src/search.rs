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
    pub(crate) features: SearchFeatures,
    pub(crate) cache: RefCell<Option<farzin::caissa::CachedEvaluator>>,
    #[cfg(feature = "nnue-stack")]
    pub(crate) stack: RefCell<Option<farzin::caissa::SearchEvaluator>>,
}

impl SearchControl {
    #[cfg(feature = "nnue-stack")]
    fn push(&self, board: &Board) {
        if let Some(stack) = self.stack.borrow_mut().as_mut() {
            stack.push(board);
        }
    }
    #[cfg(feature = "nnue-stack")]
    fn pop(&self) {
        if let Some(stack) = self.stack.borrow_mut().as_mut() {
            stack.pop();
        }
    }
    fn evaluate(&self, board: &Board) -> i32 {
        #[cfg(feature = "nnue-stack")]
        if let Some(stack) = self.stack.borrow_mut().as_mut() {
            return stack.evaluate(board);
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

pub(crate) fn quiescence(
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
            return Some(control.evaluate(&board));
        }
        // At the hard limit, examine every legal evasion once rather than
        // pretending that passing in check is legal or aborting the iteration.
        for mv in MoveGen::new_legal(&board) {
            if !control.visit_node(nodes) {
                return None;
            }
            let halfmove = game.halfmove;
            game.play(mv);
            #[cfg(feature = "nnue-stack")]
            control.push(&game.board);
            let score = match game.board.status() {
                BoardStatus::Checkmate => MATE - ply - 1,
                BoardStatus::Stalemate => 0,
                BoardStatus::Ongoing if game.draw() => 0,
                BoardStatus::Ongoing => -control.evaluate(&game.board),
            };
            #[cfg(feature = "nnue-stack")]
            control.pop();
            game.unplay(board, halfmove);
            alpha = alpha.max(score);
            if alpha >= beta {
                break;
            }
        }
        return Some(alpha);
    }
    if !in_check {
        let stand_pat = control.evaluate(&board);
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
        #[cfg(feature = "nnue-stack")]
        control.push(&game.board);
        let result = quiescence(game, -beta, -alpha, ply + 1, qply + 1, control, nodes);
        #[cfg(feature = "nnue-stack")]
        control.pop();
        game.unplay(board, halfmove);
        let score = -result?;
        if score >= beta {
            return Some(score);
        }
        alpha = alpha.max(score);
    }
    Some(alpha)
}

/// Opt-in search experiments. Every flag defaults to `false`, and with all
/// flags off the search is node-for-node identical to the Phase 3 baseline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SearchFeatures {
    /// Keep killers and a bounded, aged history table across iterative
    /// deepening iterations and aspiration re-searches of one `go`.
    pub(crate) persistent_history: bool,
    /// Conservative late move reductions for late quiet moves.
    pub(crate) lmr: bool,
    /// Guarded null move pruning with a zugzwang material guard.
    pub(crate) null_move: bool,
}

/// Bound of the gravity-style history update used by persistent history.
pub(crate) const HISTORY_MAX: i32 = 16_384;

/// Move-ordering state owned by one search (one `go` command).
///
/// It never outlives the search that created it, so a new game, position or
/// evaluator configuration always starts from an empty table.
pub(crate) struct Heuristics {
    killers: [[Option<ChessMove>; 2]; 128],
    history: [[i32; 64]; 64],
}

impl Heuristics {
    pub(crate) fn new() -> Self {
        Self {
            killers: [[None; 2]; 128],
            history: [[0; 64]; 64],
        }
    }
    pub(crate) fn reset(&mut self) {
        self.killers = [[None; 2]; 128];
        self.history = [[0; 64]; 64];
    }
    /// Halve history between iterations so recent iterations dominate.
    pub(crate) fn age(&mut self) {
        for row in &mut self.history {
            for value in row {
                *value /= 2;
            }
        }
    }
    #[cfg(test)]
    pub(crate) fn max_abs_history(&self) -> i32 {
        self.history
            .iter()
            .flatten()
            .map(|value| value.abs())
            .max()
            .unwrap_or(0)
    }
}

// Gravity update: the entry moves toward +/-HISTORY_MAX and can never leave
// the interval for |bonus| <= HISTORY_MAX.
fn update_history(entry: &mut i32, bonus: i32) {
    let bonus = bonus.clamp(-HISTORY_MAX, HISTORY_MAX);
    *entry += bonus - *entry * bonus.abs() / HISTORY_MAX;
}

fn has_non_pawn_material(board: &Board) -> bool {
    let own = *board.color_combined(board.side_to_move());
    let minor_or_major = *board.pieces(Piece::Knight)
        | *board.pieces(Piece::Bishop)
        | *board.pieces(Piece::Rook)
        | *board.pieces(Piece::Queen);
    (own & minor_or_major).popcnt() > 0
}

/// Reduction in plies for a late quiet move. Always leaves at least one ply.
pub(crate) fn lmr_reduction(depth: u8, move_index: usize, pv_node: bool) -> u8 {
    if depth < 3 || move_index < 3 {
        return 0;
    }
    let base = 0.5 + f64::from(depth).ln() * (move_index as f64 + 1.0).ln() / 2.5;
    let mut reduction = base as i32 - i32::from(pv_node);
    reduction = reduction.clamp(0, i32::from(depth) - 2);
    reduction as u8
}

struct SearchState<'a> {
    hints: &'a mut HashMap<u64, ChessMove>,
    table: &'a mut Table,
    nodes: &'a mut u64,
    heuristics: &'a mut Heuristics,
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

#[allow(clippy::too_many_arguments)]
fn search(
    game: &mut Game,
    depth: u8,
    mut alpha: i32,
    beta: i32,
    ply: i32,
    allow_null: bool,
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
    let in_check = board.checkers().popcnt() > 0;
    let pv_node = alpha + 1 < beta;
    let features = control.features;
    // Null move pruning: only in zero-window nodes, never in check, never
    // twice in a row, never near mate bounds and never without a piece other
    // than pawns (the classic zugzwang guard).
    if features.null_move
        && allow_null
        && !pv_node
        && !in_check
        && depth >= 3
        && beta.abs() < MATE - 100
        && has_non_pawn_material(&board)
        && control.evaluate(&board) >= beta
    {
        if let Some(null_board) = board.null_move() {
            let reduction = if depth >= 7 { 3 } else { 2 };
            let child_depth = depth - 1 - reduction;
            let saved = game.play_null(null_board);
            #[cfg(feature = "nnue-stack")]
            control.push(&game.board);
            let result = search(
                game,
                child_depth,
                -beta,
                -beta + 1,
                ply + 1,
                false,
                control,
                state,
            );
            #[cfg(feature = "nnue-stack")]
            control.pop();
            game.unplay_null(board, saved);
            if -result? >= beta {
                // Deep nodes are verified with a reduced normal search of the
                // same node, so a zugzwang at depth cannot be hidden by the
                // null move alone. Unproven mate scores are never returned.
                if depth < 8 {
                    return Some(beta);
                }
                let verified = search(
                    game,
                    child_depth,
                    beta - 1,
                    beta,
                    ply,
                    false,
                    control,
                    state,
                )?;
                if verified >= beta {
                    return Some(beta);
                }
            }
            state.pv_len[pv_index] = 0;
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
    let killers = state.heuristics.killers[(ply as usize).min(127)];
    let mut scores = [0; 256];
    for index in 0..move_count {
        scores[index] = move_score(
            &board,
            moves[index],
            hint,
            killers,
            &state.heuristics.history,
            control.exchange_ordering,
        );
    }
    let mut quiets_tried = [first; 64];
    let mut quiet_count = 0;
    let mut best = None;
    let mut best_score = -MATE;
    for index in 0..move_count {
        let best_index = (index..move_count).max_by_key(|&i| scores[i]).unwrap();
        moves.swap(index, best_index);
        scores.swap(index, best_index);
        let mv = moves[index];
        let quiet = capture_value(&board, mv).is_none() && mv.get_promotion().is_none();
        let halfmove = game.halfmove;
        game.play(mv);
        #[cfg(feature = "nnue-stack")]
        control.push(&game.board);
        let reduction = if features.lmr
            && quiet
            && !in_check
            && Some(mv) != hint
            && !killers.contains(&Some(mv))
            && game.board.checkers().popcnt() == 0
        {
            lmr_reduction(depth, index, pv_node)
        } else {
            0
        };
        let mut result = None;
        let mut needs_full_depth = true;
        if reduction > 0 {
            // Reduced zero-window probe; only a fail high is re-searched at
            // full depth, so a reduction can never raise alpha by itself.
            let probe = search(
                game,
                depth - 1 - reduction,
                -alpha - 1,
                -alpha,
                ply + 1,
                true,
                control,
                state,
            );
            match probe {
                Some(score) if -score > alpha => {}
                other => {
                    result = other;
                    needs_full_depth = false;
                }
            }
        }
        if needs_full_depth {
            result = if control.pvs && index > 0 && alpha + 1 < beta {
                match search(
                    game,
                    depth - 1,
                    -alpha - 1,
                    -alpha,
                    ply + 1,
                    true,
                    control,
                    state,
                ) {
                    Some(probe) if -probe > alpha && -probe < beta => search(
                        game,
                        depth - 1,
                        -beta,
                        -alpha,
                        ply + 1,
                        true,
                        control,
                        state,
                    ),
                    other => other,
                }
            } else {
                search(
                    game,
                    depth - 1,
                    -beta,
                    -alpha,
                    ply + 1,
                    true,
                    control,
                    state,
                )
            };
        }
        #[cfg(feature = "nnue-stack")]
        control.pop();
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
            if quiet {
                let killers = &mut state.heuristics.killers[(ply as usize).min(127)];
                if killers[0] != Some(mv) {
                    killers[1] = killers[0];
                    killers[0] = Some(mv);
                }
                let history = &mut state.heuristics.history;
                if features.persistent_history {
                    let bonus = (i32::from(depth) * i32::from(depth)).min(1_600);
                    update_history(
                        &mut history[mv.get_source().to_index()][mv.get_dest().to_index()],
                        bonus,
                    );
                    for tried in &quiets_tried[..quiet_count] {
                        update_history(
                            &mut history[tried.get_source().to_index()]
                                [tried.get_dest().to_index()],
                            -bonus,
                        );
                    }
                } else {
                    let entry = &mut history[mv.get_source().to_index()][mv.get_dest().to_index()];
                    *entry = entry.saturating_add(i32::from(depth).pow(2)).min(70_000);
                }
            }
            break;
        }
        if quiet && quiet_count < quiets_tried.len() {
            quiets_tried[quiet_count] = mv;
            quiet_count += 1;
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

#[cfg(test)]
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

#[cfg(test)]
pub(crate) fn best_move_window(
    game: &mut Game,
    depth: u8,
    control: &SearchControl,
    hints: &mut HashMap<u64, ChessMove>,
    table: &mut Table,
    nodes: &mut u64,
    window: (i32, i32),
) -> Option<(ChessMove, i32, Vec<ChessMove>)> {
    root_search(
        game,
        depth,
        control,
        hints,
        table,
        nodes,
        &mut Heuristics::new(),
        window,
    )
}

/// Root search with caller-owned heuristics; used by iterative deepening.
#[allow(clippy::too_many_arguments)]
pub(crate) fn root_search(
    game: &mut Game,
    depth: u8,
    control: &SearchControl,
    hints: &mut HashMap<u64, ChessMove>,
    table: &mut Table,
    nodes: &mut u64,
    heuristics: &mut Heuristics,
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
        heuristics,
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
        #[cfg(feature = "nnue-stack")]
        control.push(&game.board);
        let result = search(
            game,
            depth.saturating_sub(1),
            -beta,
            -alpha,
            1,
            true,
            control,
            &mut state,
        );
        #[cfg(feature = "nnue-stack")]
        control.pop();
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

    #[cfg(feature = "nnue-stack")]
    #[test]
    fn nnue_stack_search_cutoffs_and_interruption_restore_root() {
        let mut bytes = vec![0; 75_703_872];
        bytes[..4].copy_from_slice(&0x43534e4eu32.to_le_bytes());
        bytes[4..8].copy_from_slice(&15u32.to_le_bytes());
        for (i, byte) in bytes[64..].iter_mut().enumerate() {
            *byte = (i * 13 + 17) as u8;
        }
        let net = Arc::new(farzin::caissa::Network::from_bytes(bytes).unwrap());
        let feature_sets = [
            SearchFeatures::default(),
            SearchFeatures {
                persistent_history: true,
                lmr: true,
                null_move: true,
            },
        ];
        for (features, limit) in feature_sets
            .into_iter()
            .flat_map(|f| [None, Some(1), Some(25), Some(300)].map(move |l| (f, l)))
        {
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
                    features,
                    cache: RefCell::new((!stack_enabled).then(|| {
                        farzin::caissa::CachedEvaluator::new(Arc::clone(&net), &game.board)
                    })),
                    #[cfg(feature = "nnue-stack")]
                    stack: RefCell::new(stack_enabled.then(|| {
                        farzin::caissa::SearchEvaluator::new(Arc::clone(&net), &game.board)
                    })),
                };
                let mut nodes = 0;
                let result = best_move(
                    &mut game,
                    5,
                    &control,
                    &mut HashMap::new(),
                    &mut Table::new(1024),
                    &mut nodes,
                );
                assert_eq!(game.board, Board::default());
                assert_eq!(game.history, original);
                assert_eq!(control.evaluate(&game.board), net.evaluate(&game.board));
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
                features: SearchFeatures::default(),
                cache: RefCell::new(None),
                #[cfg(feature = "nnue-stack")]
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
                    features: SearchFeatures::default(),
                    cache: RefCell::new(None),
                    #[cfg(feature = "nnue-stack")]
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
            features: SearchFeatures::default(),
            cache: RefCell::new(None),
            #[cfg(feature = "nnue-stack")]
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
            features: SearchFeatures::default(),
            cache: RefCell::new(None),
            #[cfg(feature = "nnue-stack")]
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
                heuristics: &mut Heuristics::new(),
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
                features: SearchFeatures::default(),
                cache: RefCell::new(None),
                #[cfg(feature = "nnue-stack")]
                stack: RefCell::new(None),
            };
            assert!(search(&mut game, 1, -50, 50, 0, true, &control, &mut state).is_some());
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
            heuristics: &mut Heuristics::new(),
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
            features: SearchFeatures::default(),
            cache: RefCell::new(None),
            #[cfg(feature = "nnue-stack")]
            stack: RefCell::new(None),
        };
        assert!(search(&mut game, 4, -MATE, MATE, 0, true, &control, &mut state).is_none());
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
            heuristics: &mut Heuristics::new(),
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
            features: SearchFeatures::default(),
            cache: RefCell::new(None),
            #[cfg(feature = "nnue-stack")]
            stack: RefCell::new(None),
        };
        assert!(search(&mut game, 0, -MATE, MATE, 0, true, &control, &mut state).is_some());
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
                    features: SearchFeatures::default(),
                    cache: RefCell::new(None),
                    #[cfg(feature = "nnue-stack")]
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
                    features: SearchFeatures::default(),
                    cache: RefCell::new(None),
                    #[cfg(feature = "nnue-stack")]
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
            heuristics: &mut Heuristics::new(),
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
            features: SearchFeatures::default(),
            cache: RefCell::new(None),
            #[cfg(feature = "nnue-stack")]
            stack: RefCell::new(None),
        };
        let score = search(&mut game, 1, -MATE, MATE, 0, true, &control, &mut state).unwrap();
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
                    features: SearchFeatures::default(),
                    cache: RefCell::new(None),
                    #[cfg(feature = "nnue-stack")]
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
                    features: SearchFeatures::default(),
                    cache: RefCell::new(None),
                    #[cfg(feature = "nnue-stack")]
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
            features: SearchFeatures::default(),
            cache: RefCell::new(None),
            #[cfg(feature = "nnue-stack")]
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

    fn feature_control(
        features: SearchFeatures,
        pvs: bool,
        node_limit: Option<u64>,
    ) -> SearchControl {
        SearchControl {
            stop: Arc::new(AtomicBool::new(false)),
            deadline: None,
            node_limit,
            shared_nodes: None,
            exchange_ordering: false,
            pvs,
            positional: false,
            features,
            cache: RefCell::new(None),
            #[cfg(feature = "nnue-stack")]
            stack: RefCell::new(None),
        }
    }

    /// Every combination in release tests; a representative subset in debug
    /// builds (CI), where the full matrix takes several minutes.
    fn all_feature_sets() -> Vec<(SearchFeatures, bool)> {
        let mut sets = Vec::new();
        let subset: &[u8] = &[0, 2, 4 | 8, 2 | 8, 1, 15];
        let bits: Vec<u8> = if cfg!(debug_assertions) {
            subset.to_vec()
        } else {
            (0..16).collect()
        };
        for bits in bits {
            sets.push((
                SearchFeatures {
                    persistent_history: bits & 1 != 0,
                    lmr: bits & 2 != 0,
                    null_move: bits & 4 != 0,
                },
                bits & 8 != 0,
            ));
        }
        sets
    }

    /// Iterative deepening with one heuristics object, as the UCI loop uses it,
    /// including an aspiration trial and full-window re-search.
    fn iterate(
        game: &mut Game,
        depth: u8,
        control: &SearchControl,
        aspiration: bool,
    ) -> Option<(ChessMove, i32, Vec<ChessMove>)> {
        let mut hints = HashMap::new();
        let mut table = Table::new(1 << 14);
        let mut heuristics = Heuristics::new();
        let mut nodes = 0;
        let mut result = None;
        let mut previous: Option<i32> = None;
        for current in 1..=depth {
            if control.features.persistent_history {
                heuristics.age();
            } else {
                heuristics.reset();
            }
            table.next_generation();
            let window = previous
                .filter(|score| aspiration && score.abs() < MATE - 100)
                .map_or((-MATE, MATE), |s| (s - 30, s + 30));
            let mut found = root_search(
                game,
                current,
                control,
                &mut hints,
                &mut table,
                &mut nodes,
                &mut heuristics,
                window,
            );
            if let Some((_, score, _)) = found {
                if score <= window.0 || score >= window.1 {
                    if !control.features.persistent_history {
                        heuristics.reset();
                    }
                    found = root_search(
                        game,
                        current,
                        control,
                        &mut hints,
                        &mut table,
                        &mut nodes,
                        &mut heuristics,
                        (-MATE, MATE),
                    );
                }
            }
            match found {
                Some(found) => {
                    previous = Some(found.1);
                    result = Some(found);
                }
                None => break,
            }
            assert!(heuristics.max_abs_history() <= 70_000);
        }
        result
    }

    fn assert_legal_pv(game: &Game, pv: &[ChessMove]) {
        let mut board = game.board;
        for &mv in pv {
            assert!(
                MoveGen::new_legal(&board).any(|legal| legal == mv),
                "{pv:?}"
            );
            board = board.make_move_new(mv);
        }
    }

    #[test]
    fn history_gravity_is_bounded() {
        let mut entry = 0;
        for step in 0..10_000 {
            let bonus = ((step * 7919) % 3201) - 1600;
            update_history(&mut entry, bonus * if step % 3 == 0 { 1 } else { -1 });
            assert!(entry.abs() <= HISTORY_MAX, "{entry}");
        }
        let mut saturated = HISTORY_MAX;
        update_history(&mut saturated, HISTORY_MAX);
        assert_eq!(saturated, HISTORY_MAX);
        let mut heuristics = Heuristics::new();
        heuristics.history[1][2] = 1001;
        heuristics.history[3][4] = -7;
        heuristics.age();
        assert_eq!(heuristics.history[1][2], 500);
        assert_eq!(heuristics.history[3][4], -3);
        heuristics.reset();
        assert_eq!(heuristics.max_abs_history(), 0);
    }

    #[test]
    fn lmr_never_reduces_to_zero_depth() {
        for depth in 0..=64u8 {
            for index in 0..256 {
                for pv in [false, true] {
                    let r = lmr_reduction(depth, index, pv);
                    if depth < 3 || index < 3 {
                        assert_eq!(r, 0);
                    } else {
                        assert!(r <= depth - 2, "depth {depth} index {index}");
                    }
                    assert!(
                        lmr_reduction(depth, index, true) <= lmr_reduction(depth, index, false)
                    );
                }
            }
        }
        assert!(lmr_reduction(12, 30, false) >= 2);
    }

    #[test]
    fn null_move_is_a_repetition_barrier_and_restores_state() {
        let mut game =
            crate::game::position("startpos moves g1f3 g8f6 f3g1 f6g8 g1f3 g8f6 f3g1").unwrap();
        let before = game.clone();
        let board = game.board;
        // Black can repeat for the third time by f6g8.
        game.play(ChessMove::from_str("f6g8").unwrap());
        assert!(game.draw());
        game.unplay(board, before.halfmove);
        let saved = game.play_null(board.null_move().unwrap());
        assert!(!game.draw(), "positions before a null move must not count");
        assert_eq!(game.halfmove, before.halfmove + 1);
        game.unplay_null(board, saved);
        assert_eq!(game.board, before.board);
        assert_eq!(game.history, before.history);
        assert_eq!(
            (game.halfmove, game.barrier),
            (before.halfmove, before.barrier)
        );
    }

    #[test]
    fn null_move_guard_leaves_pawn_endings_unchanged() {
        // Zugzwang-sensitive king and pawn endings, including a rook pawn
        // and the classic mutual zugzwang trebuchet.
        for fen in [
            "fen 8/8/8/8/1k6/8/1P6/1K6 w - - 0 1",
            "fen 8/8/1p6/1P6/1K6/8/8/k7 b - - 0 1",
            "fen 7k/8/6KP/8/8/8/8/8 w - - 0 1",
            "fen 8/8/8/2k5/2p5/2K5/3P4/8 w - - 0 1",
            "fen 8/8/8/1k6/3K4/8/1P6/8 w - - 0 1",
        ] {
            let mut results = Vec::new();
            for null_move in [false, true] {
                let mut game = crate::game::position(fen).unwrap();
                let control = feature_control(
                    SearchFeatures {
                        null_move,
                        ..SearchFeatures::default()
                    },
                    true,
                    None,
                );
                let mut nodes = 0;
                let result = root_search(
                    &mut game,
                    7,
                    &control,
                    &mut HashMap::new(),
                    &mut Table::new(1 << 14),
                    &mut nodes,
                    &mut Heuristics::new(),
                    (-MATE, MATE),
                );
                results.push((result, nodes));
            }
            assert_eq!(results[0], results[1], "{fen}");
        }
    }

    #[test]
    fn null_move_is_not_tried_in_check_or_low_material() {
        let in_check = crate::game::position("fen 4k3/8/8/8/8/8/4r3/4K2R w - - 0 1").unwrap();
        assert!(in_check.board.null_move().is_none());
        let pawns = crate::game::position("fen 4k3/pp6/8/8/8/8/PP6/4K3 w - - 0 1").unwrap();
        assert!(!has_non_pawn_material(&pawns.board));
        let knight = crate::game::position("fen 4k3/pp6/8/8/8/8/PP6/3NK3 w - - 0 1").unwrap();
        assert!(has_non_pawn_material(&knight.board));
        // The opponent's pieces do not count for the side to move.
        let theirs = crate::game::position("fen 3qk3/pp6/8/8/8/8/PP6/4K3 w - - 0 1").unwrap();
        assert!(!has_non_pawn_material(&theirs.board));
    }

    #[test]
    fn all_feature_sets_solve_reference_positions() {
        // (position, depth, acceptable best moves, expected mate in moves)
        let cases: [(&str, u8, &[&str], Option<i32>); 6] = [
            // Back-rank mate in one.
            (
                "fen 6k1/5ppp/8/8/8/8/5PPP/R5K1 w - - 0 1",
                3,
                &["a1a8"],
                Some(1),
            ),
            // Mate in three by a king hunt: Bc5+ Kxc5 Qb6+ Kd5 Qd6#.
            (
                "fen r1b1kb1r/pppp1ppp/5q2/4n3/3KP3/2N3PN/PPP4P/R1BQ1B1R b kq - 0 1",
                5,
                &["f8c5"],
                Some(3),
            ),
            // Win the queen with a knight fork.
            ("fen 4k3/8/q7/1N6/8/8/8/4K3 w - - 0 1", 4, &["b5c7"], None),
            // Check evasion with only legal king moves.
            ("fen 4k3/8/8/8/8/8/3q4/4K3 w - - 0 1", 4, &["e1d2"], None),
            // Passed pawn must promote.
            ("fen 8/P7/8/8/8/8/k7/7K w - - 0 1", 4, &["a7a8q"], None),
            // Quiet middlegame: any legal move, PV legality is checked.
            (
                "fen r1bq1rk1/ppp2ppp/2n2n2/3pp3/3PP3/2N2N2/PPP2PPP/R1BQ1RK1 w - - 0 6",
                5,
                &[],
                None,
            ),
        ];
        for (fen, depth, moves, mate) in cases {
            for (features, pvs) in all_feature_sets() {
                for aspiration in [false, true] {
                    let mut game = crate::game::position(fen).unwrap();
                    let before = game.clone();
                    let control = feature_control(features, pvs, None);
                    let (best, score, pv) =
                        iterate(&mut game, depth, &control, aspiration).unwrap();
                    let label = format!("{fen} {features:?} pvs {pvs} asp {aspiration}");
                    assert_eq!(game.board, before.board, "{label}");
                    assert_eq!(game.history, before.history, "{label}");
                    assert_eq!(pv.first(), Some(&best), "{label}");
                    assert_legal_pv(&game, &pv);
                    if !moves.is_empty() {
                        assert!(
                            moves.contains(&best.to_string().as_str()),
                            "{label}: {best}"
                        );
                    }
                    if let Some(n) = mate {
                        assert_eq!(score, MATE - (2 * n - 1), "{label}");
                    }
                }
            }
        }
    }

    #[test]
    fn interrupted_searches_restore_state_for_every_feature_set() {
        let fen = "fen r1bq1rk1/ppp2ppp/2n2n2/3pp3/3PP3/2N2N2/PPP2PPP/R1BQ1RK1 w - - 0 6";
        for (features, pvs) in all_feature_sets() {
            for limit in [1, 7, 50, 400, 3000] {
                let mut game = crate::game::position(fen).unwrap();
                let before = game.clone();
                let control = feature_control(features, pvs, Some(limit));
                if let Some((best, _, pv)) = iterate(&mut game, 8, &control, true) {
                    assert_eq!(pv.first(), Some(&best));
                    assert_legal_pv(&game, &pv);
                }
                assert_eq!(game.board, before.board);
                assert_eq!(game.history, before.history);
                assert_eq!(
                    (game.halfmove, game.barrier),
                    (before.halfmove, before.barrier)
                );
            }
        }
    }

    #[test]
    fn persistent_history_preserves_minimax_scores() {
        // Without pruning or a table, ordering cannot change alpha-beta values.
        for fen in [
            "startpos",
            "fen 8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
            "fen r1bq1rk1/ppp2ppp/2n2n2/3pp3/3PP3/2N2N2/PPP2PPP/R1BQ1RK1 w - - 0 6",
        ] {
            let mut game = crate::game::position(fen).unwrap();
            let mut heuristics = Heuristics::new();
            for depth in 1..=3 {
                let mut scores = Vec::new();
                for persistent_history in [false, true] {
                    let control = feature_control(
                        SearchFeatures {
                            persistent_history,
                            ..SearchFeatures::default()
                        },
                        false,
                        None,
                    );
                    let mut fresh = Heuristics::new();
                    let heur = if persistent_history {
                        &mut heuristics
                    } else {
                        &mut fresh
                    };
                    heur.age();
                    scores.push(
                        root_search(
                            &mut game,
                            depth,
                            &control,
                            &mut HashMap::new(),
                            &mut Table::new(0),
                            &mut 0,
                            heur,
                            (-MATE, MATE),
                        )
                        .unwrap()
                        .1,
                    );
                }
                assert_eq!(scores[0], scores[1], "{fen} depth {depth}");
            }
        }
    }

    #[test]
    fn repetition_draw_is_found_with_pruning() {
        // White is lost on material but can repeat: the third occurrence draws.
        let game = crate::game::position(
            "fen 6k1/8/8/8/8/8/q7/6K1 b - - 0 1 moves a2b1 g1g2 b1a2 g2g1 a2b1 g1g2 b1a2",
        )
        .unwrap();
        for (features, pvs) in all_feature_sets() {
            let mut game = game.clone();
            let control = feature_control(features, pvs, None);
            let (best, score, _) = iterate(&mut game, 4, &control, false).unwrap();
            assert_eq!(best.to_string(), "g2g1", "{features:?}");
            assert_eq!(score, 0, "{features:?}");
        }
    }
}
