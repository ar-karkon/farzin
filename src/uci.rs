use crate::tt::Table;
use chess::{Board, MoveGen};
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const MATE: i32 = 30_000;

use crate::game::{position, Game};
use crate::search::{best_move, best_move_window, SearchControl};
type Worker = (Arc<AtomicBool>, Arc<AtomicBool>, JoinHandle<()>);

#[derive(Clone, Copy)]
struct SearchOptions {
    hash_mb: usize,
    overhead: u64,
    exchange_ordering: bool,
    pvs: bool,
    aspiration: bool,
    positional: bool,
}

struct IterationState {
    hints: HashMap<u64, chess::ChessMove>,
    table: Table,
    nodes: u64,
    previous_score: Option<i32>,
}

impl IterationState {
    fn new(entries: usize) -> Self {
        Self {
            hints: HashMap::new(),
            table: Table::new(entries),
            nodes: 0,
            previous_score: None,
        }
    }

    fn search(
        &mut self,
        game: &mut Game,
        depth: u8,
        control: &SearchControl,
        aspiration: bool,
    ) -> Option<(chess::ChessMove, i32, Vec<chess::ChessMove>)> {
        self.table.next_generation();
        let window = self
            .previous_score
            .filter(|score| aspiration && score.abs() < MATE - 100)
            .map(|score| ((score - 50).max(-MATE), (score + 50).min(MATE)));
        let result = if let Some(bounds) = window {
            let trial = best_move_window(
                game,
                depth,
                control,
                &mut self.hints,
                &mut self.table,
                &mut self.nodes,
                bounds,
            );
            match trial {
                Some((_, score, _)) if score <= bounds.0 || score >= bounds.1 => best_move(
                    game,
                    depth,
                    control,
                    &mut self.hints,
                    &mut self.table,
                    &mut self.nodes,
                ),
                _ => trial,
            }
        } else {
            best_move(
                game,
                depth,
                control,
                &mut self.hints,
                &mut self.table,
                &mut self.nodes,
            )
        };
        if let Some((_, score, _)) = &result {
            self.previous_score = Some(*score);
        }
        result
    }
}

fn stop_search(worker: &mut Option<Worker>, discard: bool) {
    if let Some((stop, suppress, handle)) = worker.take() {
        if discard {
            suppress.store(true, Ordering::Relaxed);
        }
        stop.store(true, Ordering::Relaxed);
        let _ = handle.join();
    }
}

fn start_search(
    mut game: Game,
    args: &str,
    options: SearchOptions,
    network: Option<Arc<farzin::caissa::Network>>,
) -> Worker {
    let words: Vec<_> = args.split_whitespace().collect();
    let number = |key: &str| {
        words
            .windows(2)
            .find(|w| w[0] == key)
            .and_then(|w| w[1].parse::<u64>().ok())
    };
    let millis = crate::time::budget_ms(&words, game.board.side_to_move(), options.overhead);
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
        shared_nodes: None,
        exchange_ordering: options.exchange_ordering,
        pvs: options.pvs,
        positional: options.positional,
        cache: RefCell::new(
            network.map(|net| farzin::caissa::CachedEvaluator::new(net, &game.board)),
        ),
    };
    let handle = thread::spawn(move || {
        let mut best = MoveGen::new_legal(&game.board).next();
        let mut state = IterationState::new(Table::entries_for_megabytes(options.hash_mb));
        for current_depth in 1..=depth {
            let result = state.search(&mut game, current_depth, &control, options.aspiration);
            if let Some((mv, score, pv)) = result {
                best = Some(mv);
                let uci_score = if score.abs() > MATE - 100 {
                    let moves = (MATE - score.abs() + 1) / 2;
                    format!("mate {}", if score > 0 { moves } else { -moves })
                } else {
                    format!("cp {score}")
                };
                let elapsed = started.elapsed().as_millis();
                let nodes = state.nodes;
                let nps = (nodes as u128 * 1000) / elapsed.max(1);
                if !suppress_worker.load(Ordering::Relaxed) {
                    let pv = pv
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(" ");
                    println!("info depth {current_depth} score {uci_score} nodes {nodes} nps {nps} time {elapsed} hashfull {} pv {pv}", state.table.hashfull());
                }
            } else {
                break;
            }
        }
        if !suppress_worker.load(Ordering::Relaxed) {
            println!(
                "info string tt probes {} hits {} cutoffs {}",
                state.table.probes, state.table.hits, state.table.cutoffs
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

pub(crate) fn run() {
    let stdin = io::stdin();
    let mut game = Game::new(Board::default(), 0);
    let mut worker = None;
    let mut options = SearchOptions {
        hash_mb: 16,
        overhead: 20,
        exchange_ordering: false,
        pvs: false,
        aspiration: false,
        positional: false,
    };
    let mut network = None;
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line == "uci" {
            println!("id name Farzin 0.1.0\nid author Farzin contributors\noption name Hash type spin default 16 min 1 max 256\noption name Clear Hash type button\noption name Move Overhead type spin default 20 min 0 max 5000\noption name Exchange Ordering type check default false\noption name PVS type check default false\noption name Aspiration type check default false\noption name Positional type check default false\noption name EvalFile type string default <empty>\nuciok");
        } else if line == "isready" {
            println!("readyok");
        } else if line == "ucinewgame" {
            stop_search(&mut worker, true);
            game = Game::new(Board::default(), 0);
        } else if let Some(value) = line.strip_prefix("setoption name Hash value ") {
            if let Ok(value) = value.parse::<usize>() {
                options.hash_mb = value.clamp(1, 256);
            }
        } else if line == "setoption name Clear Hash" {
            // Tables are per-search, so no state survives a completed search.
        } else if let Some(value) = line.strip_prefix("setoption name Move Overhead value ") {
            if let Ok(value) = value.parse::<u64>() {
                options.overhead = value.min(5000);
            }
        } else if let Some(value) = line.strip_prefix("setoption name Exchange Ordering value ") {
            if value == "true" || value == "false" {
                options.exchange_ordering = value == "true";
            }
        } else if let Some(value) = line.strip_prefix("setoption name PVS value ") {
            if value == "true" || value == "false" {
                options.pvs = value == "true";
            }
        } else if let Some(value) = line.strip_prefix("setoption name Aspiration value ") {
            if value == "true" || value == "false" {
                options.aspiration = value == "true";
            }
        } else if let Some(value) = line.strip_prefix("setoption name Positional value ") {
            if value == "true" || value == "false" {
                options.positional = value == "true";
            }
        } else if let Some(value) = line.strip_prefix("setoption name EvalFile value ") {
            stop_search(&mut worker, true);
            if value.is_empty() || value == "<empty>" {
                network = None;
                println!("info string classical evaluator enabled");
            } else {
                match farzin::caissa::Network::from_file(value) {
                    Ok(loaded) => {
                        network = Some(Arc::new(loaded));
                        println!("info string experimental network loaded");
                    }
                    Err(error) => println!("info string failed to load network: {error}"),
                }
            }
        } else if let Some(args) = line.strip_prefix("position ") {
            stop_search(&mut worker, true);
            match position(args) {
                Ok(next) => game = next,
                Err(e) => println!("info string {e}"),
            }
        } else if line == "go" || line.starts_with("go ") {
            stop_search(&mut worker, true);
            worker = Some(start_search(game.clone(), line, options, network.clone()));
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
    use crate::search::{quiescence, MAX_QPLY};
    use chess::{BoardStatus, ChessMove};
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
    fn kiwipete_perft() {
        let board =
            position("fen r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1")
                .unwrap()
                .board;
        assert_eq!(perft(&board, 1), 48);
        assert_eq!(perft(&board, 2), 2039);
        assert_eq!(perft(&board, 3), 97862);
    }

    #[test]
    fn perft_promotion_and_castling_positions() {
        // Chess Programming Wiki, Perft Results: positions 4 and 5.
        let cases = [
            (
                "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
                [6, 264, 9467],
            ),
            (
                "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
                [44, 1486, 62379],
            ),
        ];
        for (fen, expected) in cases {
            let game = position(&format!("fen {fen}")).unwrap();
            for depth in 1..=3 {
                assert_eq!(
                    perft(&game.board, depth),
                    expected[usize::from(depth - 1)],
                    "{fen} depth {depth}"
                );
            }
        }
    }
    #[test]
    fn recognizes_stalemate() {
        let game = position("fen 7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").unwrap();
        assert_eq!(game.board.status(), BoardStatus::Stalemate);
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
            shared_nodes: None,
            exchange_ordering: false,
            pvs: false,
            positional: false,
            cache: RefCell::new(None),
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
            shared_nodes: None,
            exchange_ordering: false,
            pvs: false,
            positional: false,
            cache: RefCell::new(None),
        };
        assert!(quiescence(&mut game, -MATE, MATE, 0, MAX_QPLY, &control, &mut 0).is_some());
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
            shared_nodes: None,
            exchange_ordering: false,
            pvs: false,
            positional: false,
            cache: RefCell::new(None),
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
