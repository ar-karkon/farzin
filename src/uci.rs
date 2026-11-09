use crate::tt::Table;
use chess::{Board, Color, MoveGen};
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
use crate::search::{best_move, SearchControl};
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
        let mut table = Table::new(Table::entries_for_megabytes(hash_mb));
        let mut nodes = 0;
        for current_depth in 1..=depth {
            table.next_generation();
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

pub(crate) fn run() {
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
