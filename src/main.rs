use chess::{Board, BoardStatus, ChessMove, Color, MoveGen, Piece};
use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
mod tt;
use tt::{Bound, Entry, Table};

const MATE: i32 = 30_000;

mod evaluation;
mod game;
use evaluation::evaluate;
use game::{position, Game};
struct SearchControl {
    stop: Arc<AtomicBool>,
    deadline: Option<Instant>,
    node_limit: Option<u64>,
}

impl SearchControl {
    fn stopped(&self, nodes: u64) -> bool {
        self.stop.load(Ordering::Relaxed)
            || self.deadline.is_some_and(|d| Instant::now() >= d)
            || self.node_limit.is_some_and(|limit| nodes >= limit)
    }
}

const MAX_QPLY: i32 = 16;

fn quiescence(
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

fn best_move(
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

type Worker = (Arc<AtomicBool>, Arc<AtomicBool>, JoinHandle<()>);

fn stop_search(worker: &mut Option<Worker>, discard: bool) {
    if let Some((stop, suppress, handle)) = worker.take() {
        if discard {
            suppress.store(true, Ordering::Relaxed);
        }
        stop.store(true, Ordering::Relaxed);
        let _ = handle.join();
    }
}

fn start_search(mut game: Game, args: &str, hash_mb: usize) -> Worker {
    let words: Vec<_> = args.split_whitespace().collect();
    let number = |key: &str| {
        words
            .windows(2)
            .find(|w| w[0] == key)
            .and_then(|w| w[1].parse::<u64>().ok())
    };
    let (clock, increment) = if game.board.side_to_move() == Color::White {
        (number("wtime"), number("winc"))
    } else {
        (number("btime"), number("binc"))
    };
    let clock_budget = clock.map(|remaining| {
        let moves = number("movestogo").unwrap_or(30).clamp(1, 100);
        (remaining / moves)
            .saturating_add(increment.unwrap_or(0) / 2)
            .min(remaining.saturating_sub(20))
            .max(1)
    });
    let millis = if words.contains(&"infinite") {
        None
    } else {
        number("movetime").or(clock_budget)
    };
    let depth = number("depth")
        .unwrap_or(
            if words.contains(&"infinite") || millis.is_some() || number("nodes").is_some() {
                64
            } else {
                3
            },
        )
        .clamp(1, 64) as u8;
    let time = millis.map(Duration::from_millis);
    let stop = Arc::new(AtomicBool::new(false));
    let suppress = Arc::new(AtomicBool::new(false));
    let suppress_worker = Arc::clone(&suppress);
    let started = Instant::now();
    let control = SearchControl {
        stop: Arc::clone(&stop),
        deadline: time.and_then(|t| Instant::now().checked_add(t)),
        node_limit: number("nodes"),
    };
    let handle = thread::spawn(move || {
        let mut best = MoveGen::new_legal(&game.board).next();
        let mut hints = HashMap::new();
        // HashMap overhead is implementation-dependent; reserve conservatively per entry.
        let mut table = Table::new(hash_mb.saturating_mul(1024 * 1024) / 128);
        let mut nodes = 0;
        for current_depth in 1..=depth {
            if let Some((mv, score)) = best_move(
                &mut game,
                current_depth,
                &control,
                &mut hints,
                &mut table,
                &mut nodes,
            ) {
                best = Some(mv);
                let uci_score = if score.abs() > MATE - 100 {
                    let moves = (MATE - score.abs() + 1) / 2;
                    format!("mate {}", if score > 0 { moves } else { -moves })
                } else {
                    format!("cp {score}")
                };
                let elapsed = started.elapsed().as_millis();
                let nps = (nodes as u128 * 1000) / elapsed.max(1);
                if !suppress_worker.load(Ordering::Relaxed) {
                    println!("info depth {current_depth} score {uci_score} nodes {nodes} nps {nps} time {elapsed} hashfull {} pv {mv}", table.hashfull());
                }
            } else {
                break;
            }
        }
        if !suppress_worker.load(Ordering::Relaxed) {
            println!(
                "info string tt hits {} cutoffs {}",
                table.hits, table.cutoffs
            );
            println!(
                "bestmove {}",
                best.map(|m| m.to_string()).unwrap_or_else(|| "0000".into())
            );
        }
        let _ = io::stdout().flush();
    });
    (stop, suppress, handle)
}

fn main() {
    let stdin = io::stdin();
    let mut game = Game::new(Board::default(), 0);
    let mut worker = None;
    let mut hash_mb = 16usize;
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line == "uci" {
            println!("id name Farzin 0.1.0\nid author Farzin contributors\noption name Hash type spin default 16 min 1 max 256\noption name Clear Hash type button\nuciok");
        } else if line == "isready" {
            println!("readyok");
        } else if line == "ucinewgame" {
            stop_search(&mut worker, true);
            game = Game::new(Board::default(), 0);
        } else if let Some(value) = line.strip_prefix("setoption name Hash value ") {
            if let Ok(value) = value.parse::<usize>() {
                hash_mb = value.clamp(1, 256);
            }
        } else if line == "setoption name Clear Hash" {
            // Tables are per-search, so no state survives a completed search.
        } else if let Some(args) = line.strip_prefix("position ") {
            stop_search(&mut worker, true);
            match position(args) {
                Ok(next) => game = next,
                Err(e) => println!("info string {e}"),
            }
        } else if line == "go" || line.starts_with("go ") {
            stop_search(&mut worker, true);
            worker = Some(start_search(game.clone(), line, hash_mb));
        } else if line == "stop" {
            stop_search(&mut worker, false);
        } else if line == "quit" {
            stop_search(&mut worker, true);
            break;
        }
        let _ = io::stdout().flush();
    }
    stop_search(&mut worker, true);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    fn perft(board: &Board, depth: u8) -> u64 {
        if depth == 0 {
            return 1;
        }
        MoveGen::new_legal(board)
            .map(|mv| perft(&board.make_move_new(mv), depth - 1))
            .sum()
    }
    #[test]
    fn initial_perft() {
        assert_eq!(perft(&Board::default(), 3), 8902);
    }
    #[test]
    fn endgame_perft() {
        let board = position("fen 8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1")
            .unwrap()
            .board;
        assert_eq!(perft(&board, 3), 2812);
    }
    #[test]
    fn rejects_illegal_move() {
        assert!(position("startpos moves e2e5").is_err());
    }
    #[test]
    fn repetition_and_halfmove_clock() {
        let repeated = position("startpos moves g1f3 g8f6 f3g1 f6g8 g1f3 g8f6 f3g1 f6g8").unwrap();
        assert!(repeated.draw());
        let fifty = position("fen 4k3/8/8/8/8/8/8/R3K3 w Q - 99 1 moves a1a2").unwrap();
        assert!(fifty.draw());
        let reset = position("fen 4k3/8/8/8/8/8/P7/4K3 w - - 99 1 moves a2a3").unwrap();
        assert_eq!(reset.halfmove, 0);
    }
    #[test]
    fn special_moves_are_legal() {
        assert!(position("fen r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1 moves e1g1").is_ok());
        assert!(position("fen 4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1 moves e5d6").is_ok());
        assert!(position("fen 4k3/P7/8/8/8/8/8/4K3 w - - 0 1 moves a7a8q").is_ok());
    }
    #[test]
    fn make_unmake_restores_state() {
        for (fen, mv) in [
            ("fen r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 13 1", "e1g1"),
            ("fen 4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1", "e5d6"),
            ("fen 4k3/P7/8/8/8/8/8/4K3 w - - 0 1", "a7a8n"),
        ] {
            let mut game = position(fen).unwrap();
            let old_board = game.board;
            let old_history = game.history.clone();
            let halfmove = game.halfmove;
            game.play(ChessMove::from_str(mv).unwrap());
            game.unplay(old_board, halfmove);
            assert_eq!(game.board, old_board);
            assert_eq!(game.halfmove, halfmove);
            assert_eq!(game.history, old_history);
        }
    }
    #[test]
    fn quiescence_sees_recapture() {
        let mut game = position("fen 3q3k/8/8/8/8/8/3r4/K2Q4 w - - 0 1").unwrap();
        let board = game.board;
        let control = SearchControl {
            stop: Arc::new(AtomicBool::new(false)),
            deadline: None,
            node_limit: None,
        };
        let score = quiescence(&mut game, -MATE, MATE, 0, 0, &control, &mut 0).unwrap();
        assert!(
            score < 0,
            "hanging queen capture should not win material: {score}"
        );
        assert_eq!(game.board, board);
    }
    #[test]
    fn quiescence_never_stand_pats_in_check_at_limit() {
        let mut game = position("fen 4k3/8/8/8/8/8/4r3/4K3 w - - 0 1").unwrap();
        let original = game.board;
        let control = SearchControl {
            stop: Arc::new(AtomicBool::new(false)),
            deadline: None,
            node_limit: None,
        };
        assert!(quiescence(&mut game, -MATE, MATE, 0, MAX_QPLY, &control, &mut 0).is_none());
        assert_eq!(game.board, original);
    }
    #[test]
    fn mate_has_no_move() {
        let mut board = position("startpos moves f2f3 e7e5 g2g4 d8h4").unwrap();
        assert_eq!(board.board.status(), BoardStatus::Checkmate);
        let control = SearchControl {
            stop: Arc::new(AtomicBool::new(false)),
            deadline: None,
            node_limit: None,
        };
        assert!(best_move(
            &mut board,
            2,
            &control,
            &mut HashMap::new(),
            &mut Table::new(10),
            &mut 0
        )
        .is_none());
    }
}
