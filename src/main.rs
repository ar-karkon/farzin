use chess::{Board, BoardStatus, ChessMove, Color, MoveGen, Piece};
use std::io::{self, BufRead, Write};
use std::str::FromStr;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use std::collections::HashMap;

const MATE: i32 = 30_000;

fn evaluate(board: &Board) -> i32 {
    let pieces = [(Piece::Pawn, 100), (Piece::Knight, 320), (Piece::Bishop, 330),
        (Piece::Rook, 500), (Piece::Queen, 900)];
    let mut score = 0;
    for (piece, value) in pieces {
        let white = (board.pieces(piece) & board.color_combined(Color::White)).popcnt() as i32;
        let black = (board.pieces(piece) & board.color_combined(Color::Black)).popcnt() as i32;
        score += (white - black) * value;
    }
    if board.side_to_move() == Color::White { score } else { -score }
}

struct SearchControl { stop: Arc<AtomicBool>, deadline: Option<Instant> }

#[derive(Clone)]
struct Game { board: Board, history: Vec<u64>, halfmove: u32 }

impl Game {
    fn new(board: Board, halfmove: u32) -> Self { Self { board, history: vec![board.get_hash()], halfmove } }
    fn play(&mut self, mv: ChessMove) {
        let pawn = self.board.piece_on(mv.get_source()) == Some(Piece::Pawn);
        let capture = self.board.piece_on(mv.get_dest()).is_some() || (pawn && mv.get_source().get_file() != mv.get_dest().get_file());
        self.halfmove = if pawn || capture { 0 } else { self.halfmove.saturating_add(1) };
        self.board = self.board.make_move_new(mv);
        self.history.push(self.board.get_hash());
    }
    fn unplay(&mut self, board: Board, halfmove: u32) {
        self.board = board;
        self.halfmove = halfmove;
        self.history.pop();
    }
    fn draw(&self) -> bool {
        self.halfmove >= 100 || self.history.iter().filter(|&&hash| hash == self.board.get_hash()).count() >= 3
    }
}

impl SearchControl {
    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed) || self.deadline.is_some_and(|d| Instant::now() >= d)
    }
}

fn search(game: &mut Game, depth: u8, mut alpha: i32, beta: i32, ply: i32, control: &SearchControl, hints: &mut HashMap<u64, ChessMove>, nodes: &mut u64) -> Option<i32> {
    if control.stopped() { return None; }
    *nodes += 1;
    let board = game.board;
    match board.status() {
        BoardStatus::Checkmate => return Some(-MATE + ply),
        BoardStatus::Stalemate => return Some(0),
        BoardStatus::Ongoing => {}
    }
    if game.draw() { return Some(0); }
    if depth == 0 { return Some(evaluate(&board)); }
    let mut moves: Vec<_> = MoveGen::new_legal(&board).collect();
    moves.sort_by_key(|mv| std::cmp::Reverse((hints.get(&board.get_hash()) == Some(mv), board.piece_on(mv.get_dest()).is_some(), mv.get_promotion().is_some())));
    for mv in moves {
        let halfmove = game.halfmove;
        game.play(mv);
        let result = search(game, depth - 1, -beta, -alpha, ply + 1, control, hints, nodes);
        game.unplay(board, halfmove);
        let score = -result?;
        if score > alpha {
            alpha = score;
            if hints.len() < 200_000 { hints.insert(board.get_hash(), mv); }
        }
        if alpha >= beta { break; }
    }
    Some(alpha)
}

fn best_move(game: &mut Game, depth: u8, control: &SearchControl, hints: &mut HashMap<u64, ChessMove>, nodes: &mut u64) -> Option<(ChessMove, i32)> {
    let board = game.board;
    if game.draw() { return MoveGen::new_legal(&board).next().map(|mv| (mv, 0)); }
    let mut best = None;
    let mut alpha = -MATE;
    for mv in MoveGen::new_legal(&board) {
        let halfmove = game.halfmove;
        game.play(mv);
        let result = search(game, depth.saturating_sub(1), -MATE, -alpha, 1, control, hints, nodes);
        game.unplay(board, halfmove);
        let score = -result?;
        if best.is_none() || score > alpha { best = Some(mv); alpha = score; }
    }
    best.map(|mv| (mv, alpha))
}

fn stop_search(worker: &mut Option<(Arc<AtomicBool>, JoinHandle<()>)>) {
    if let Some((stop, handle)) = worker.take() {
        stop.store(true, Ordering::Relaxed);
        let _ = handle.join();
    }
}

fn start_search(mut game: Game, args: &str) -> (Arc<AtomicBool>, JoinHandle<()>) {
    let words: Vec<_> = args.split_whitespace().collect();
    let number = |key: &str| words.windows(2).find(|w| w[0] == key).and_then(|w| w[1].parse::<u64>().ok());
    let (clock, increment) = if game.board.side_to_move() == Color::White { (number("wtime"), number("winc")) } else { (number("btime"), number("binc")) };
    let clock_budget = clock.map(|remaining| {
        let moves = number("movestogo").unwrap_or(30).clamp(1, 100);
        (remaining / moves).saturating_add(increment.unwrap_or(0) / 2).min(remaining.saturating_sub(20)).max(1)
    });
    let millis = if words.contains(&"infinite") { None } else { number("movetime").or(clock_budget) };
    let depth = number("depth").unwrap_or(if words.contains(&"infinite") || millis.is_some() { 64 } else { 3 }).clamp(1, 64) as u8;
    let time = millis.map(Duration::from_millis);
    let stop = Arc::new(AtomicBool::new(false));
    let control = SearchControl { stop: Arc::clone(&stop), deadline: time.and_then(|t| Instant::now().checked_add(t)) };
    let handle = thread::spawn(move || {
        let mut best = MoveGen::new_legal(&game.board).next();
        let mut hints = HashMap::new();
        let mut nodes = 0;
        for current_depth in 1..=depth {
            if let Some((mv, score)) = best_move(&mut game, current_depth, &control, &mut hints, &mut nodes) {
                best = Some(mv);
                let uci_score = if score.abs() > MATE - 100 {
                    let moves = (MATE - score.abs() + 1) / 2;
                    format!("mate {}", if score > 0 { moves } else { -moves })
                } else { format!("cp {score}") };
                println!("info depth {current_depth} score {uci_score} nodes {nodes} pv {mv}");
            } else { break; }
        }
        println!("bestmove {}", best.map(|m| m.to_string()).unwrap_or_else(|| "0000".into()));
        let _ = io::stdout().flush();
    });
    (stop, handle)
}

fn position(command: &str) -> Result<Game, String> {
    let mut parts = command.split_whitespace();
    let mut game = match parts.next() {
        Some("startpos") => Game::new(Board::default(), 0),
        Some("fen") => {
            let fen: Vec<_> = parts.by_ref().take(6).collect();
            if fen.len() != 6 { return Err("FEN requires six fields".into()); }
            let halfmove = fen[4].parse().map_err(|_| "invalid halfmove clock")?;
            let board = Board::from_str(&fen.join(" ")).map_err(|e| format!("invalid FEN: {e}"))?;
            Game::new(board, halfmove)
        }
        _ => return Err("expected startpos or fen".into()),
    };
    if let Some(token) = parts.next() {
        if token != "moves" { return Err("expected moves".into()); }
        for token in parts {
            let mv = ChessMove::from_str(token).map_err(|_| format!("invalid move: {token}"))?;
            if !MoveGen::new_legal(&game.board).any(|legal| legal == mv) {
                return Err(format!("illegal move: {token}"));
            }
            game.play(mv);
        }
    }
    Ok(game)
}

fn main() {
    let stdin = io::stdin();
    let mut game = Game::new(Board::default(), 0);
    let mut worker = None;
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line == "uci" {
            println!("id name Farzin 0.1.0\nid author Farzin contributors\nuciok");
        } else if line == "isready" { println!("readyok"); }
        else if line == "ucinewgame" { stop_search(&mut worker); game = Game::new(Board::default(), 0); }
        else if let Some(args) = line.strip_prefix("position ") {
            stop_search(&mut worker);
            match position(args) { Ok(next) => game = next, Err(e) => println!("info string {e}") }
        } else if line == "go" || line.starts_with("go ") {
            stop_search(&mut worker);
            worker = Some(start_search(game.clone(), line));
        } else if line == "stop" { stop_search(&mut worker); }
        else if line == "quit" { stop_search(&mut worker); break; }
        let _ = io::stdout().flush();
    }
    stop_search(&mut worker);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn perft(board: &Board, depth: u8) -> u64 {
        if depth == 0 { return 1; }
        MoveGen::new_legal(board).map(|mv| perft(&board.make_move_new(mv), depth - 1)).sum()
    }
    #[test] fn initial_perft() { assert_eq!(perft(&Board::default(), 3), 8902); }
    #[test] fn endgame_perft() {
        let board = position("fen 8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1").unwrap().board;
        assert_eq!(perft(&board, 3), 2812);
    }
    #[test] fn rejects_illegal_move() { assert!(position("startpos moves e2e5").is_err()); }
    #[test] fn repetition_and_halfmove_clock() {
        let repeated = position("startpos moves g1f3 g8f6 f3g1 f6g8 g1f3 g8f6 f3g1 f6g8").unwrap();
        assert!(repeated.draw());
        let fifty = position("fen 4k3/8/8/8/8/8/8/R3K3 w Q - 99 1 moves a1a2").unwrap();
        assert!(fifty.draw());
        let reset = position("fen 4k3/8/8/8/8/8/P7/4K3 w - - 99 1 moves a2a3").unwrap();
        assert_eq!(reset.halfmove, 0);
    }
    #[test] fn special_moves_are_legal() {
        assert!(position("fen r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1 moves e1g1").is_ok());
        assert!(position("fen 4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1 moves e5d6").is_ok());
        assert!(position("fen 4k3/P7/8/8/8/8/8/4K3 w - - 0 1 moves a7a8q").is_ok());
    }
    #[test] fn make_unmake_restores_state() {
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
    #[test] fn mate_has_no_move() {
        let mut board = position("startpos moves f2f3 e7e5 g2g4 d8h4").unwrap();
        assert_eq!(board.board.status(), BoardStatus::Checkmate);
        let control = SearchControl { stop: Arc::new(AtomicBool::new(false)), deadline: None };
        assert!(best_move(&mut board, 2, &control, &mut HashMap::new(), &mut 0).is_none());
    }
}
