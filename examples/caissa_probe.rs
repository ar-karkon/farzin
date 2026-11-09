//! Diagnostic only: print raw outputs from a local Caissa v15 network.
use chess::{Board, ChessMove, Color, MoveGen};
use farzin::caissa::{CachedEvaluator, Network, PerspectiveAccumulator, SearchEvaluator};
use std::str::FromStr;
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: caissa_probe NETWORK [FEN [MOVES...]]")?;
    let mut board = match args.next() {
        Some(fen) => Board::from_str(&fen).map_err(|e| format!("invalid FEN: {e}"))?,
        None => Board::default(),
    };
    let mut loaded = Network::from_file(path)?;
    loaded.set_simd(std::env::var_os("FARZIN_PROBE_SIMD").is_some());
    let network = Arc::new(loaded);
    let mut stack = SearchEvaluator::new(Arc::clone(&network), &board);
    let mut cache = CachedEvaluator::new(Arc::clone(&network), &board);
    let mut white = PerspectiveAccumulator::new(&network, &board, Color::White);
    let mut black = PerspectiveAccumulator::new(&network, &board, Color::Black);
    for token in args {
        let mv = ChessMove::from_str(&token).map_err(|e| format!("invalid move: {e}"))?;
        if !MoveGen::new_legal(&board).any(|legal| legal == mv) {
            return Err(format!("illegal move: {token}").into());
        }
        let next = board.make_move_new(mv);
        stack.push(&next);
        if stack.evaluate(&next) != network.evaluate(&next) {
            return Err(format!("stack score mismatch after {token}").into());
        }
        stack.pop();
        if stack.evaluate(&board) != network.evaluate(&board) {
            return Err(format!("stack undo mismatch after {token}").into());
        }
        stack.push(&next);
        white.update(&next);
        black.update(&next);
        if *white.values() != network.accumulator(&next, Color::White)
            || *black.values() != network.accumulator(&next, Color::Black)
        {
            return Err(format!("incremental mismatch after {token}").into());
        }
        if cache.evaluate(&next) != network.evaluate(&next) {
            return Err(format!("cached score mismatch after {token}").into());
        }
        if cache.evaluate(&board) != network.evaluate(&board) {
            return Err(format!("cached undo mismatch after {token}").into());
        }
        board = next;
    }
    for (bucket, output) in network.raw_outputs(&board).iter().enumerate() {
        println!("bucket={bucket} raw={output}");
    }
    eprintln!("approximate centipawns={}", network.evaluate(&board));
    Ok(())
}
