//! Diagnostic only: print raw outputs from a local Caissa v15 network.
use chess::Board;
use farzin::caissa::Network;
use std::str::FromStr;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("usage: caissa_probe NETWORK [FEN]")?;
    let board = match args.next() {
        Some(fen) => Board::from_str(&fen).map_err(|e| format!("invalid FEN: {e}"))?,
        None => Board::default(),
    };
    let network = Network::from_file(path)?;
    for (bucket, output) in network.raw_outputs(&board).iter().enumerate() {
        println!("bucket={bucket} raw={output}");
    }
    Ok(())
}
