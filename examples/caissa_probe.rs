//! Diagnostic only: print raw outputs from a local Caissa v15 network.
use chess::{Board, ChessMove, Color, MoveGen};
use farzin::caissa::{Network, PerspectiveAccumulator};
use std::str::FromStr;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: caissa_probe NETWORK [FEN [MOVES...]]")?;
    let mut board = match args.next() {
        Some(fen) => Board::from_str(&fen).map_err(|e| format!("invalid FEN: {e}"))?,
        None => Board::default(),
    };
    let network = Network::from_file(path)?;
    let mut white = PerspectiveAccumulator::new(&network, &board, Color::White);
    let mut black = PerspectiveAccumulator::new(&network, &board, Color::Black);
    for token in args {
        let mv = ChessMove::from_str(&token).map_err(|e| format!("invalid move: {e}"))?;
        if !MoveGen::new_legal(&board).any(|legal| legal == mv) {
            return Err(format!("illegal move: {token}").into());
        }
        let next = board.make_move_new(mv);
        white.update(&next);
        black.update(&next);
        if *white.values() != network.accumulator(&next, Color::White)
            || *black.values() != network.accumulator(&next, Color::Black)
        {
            return Err(format!("incremental mismatch after {token}").into());
        }
        board = next;
    }
    for (bucket, output) in network.raw_outputs(&board).iter().enumerate() {
        println!("bucket={bucket} raw={output}");
    }
    Ok(())
}
