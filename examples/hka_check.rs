//! Random-walk checker for HalfKAv2_hm inference.
//!
//! `cargo run --release --example hka_check -- NET.nnue BOOK.epd N > out.tsv`
//!
//! Plays random legal games from book positions, asserts that incremental
//! accumulators equal a from-scratch refresh at every ply, and prints
//! `fen<TAB>raw` lines for comparison against a reference engine
//! (`tools/hka_oracle.py`).
use farzin::board::Board;
use farzin::hka::{Accumulators, Network};
use farzin::movegen::legal_moves;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let net = Network::from_bytes(&std::fs::read(&args[1]).unwrap()).unwrap();
    let book = std::fs::read_to_string(&args[2]).unwrap();
    let n: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(200);
    let mut acc = Accumulators::new();
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut checked = 0usize;
    for line in book.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')).take(n) {
        let fen: String = line.split_whitespace().take(4).collect::<Vec<_>>().join(" ") + " 0 1";
        let mut board = Board::from_fen(&fen).unwrap();
        acc.refresh(&net, &board, 0);
        let plies = 20 + (rnd() % 140) as usize;
        for ply in 1..plies.min(120) {
            let moves = legal_moves(&board);
            if moves.is_empty() {
                break;
            }
            // Bias toward captures so later plies reach endgames.
            let noisy: Vec<_> = moves.iter().copied().filter(|m| m.is_noisy()).collect();
            let r = rnd();
            let mv = if !noisy.is_empty() && r % 2 == 0 {
                noisy[(r >> 8) as usize % noisy.len()]
            } else {
                moves[(r >> 8) as usize % moves.len()]
            };
            let child = board.make_move(mv);
            acc.push(&net, &board, &child, mv, ply);
            let inc = acc.evaluate(&net, &child, ply);
            let full = net.evaluate(&child);
            assert_eq!(inc, full, "incremental mismatch at {} after {}", board.to_fen(), mv.to_uci());
            checked += 1;
            if ply % 7 == 0 {
                println!("{}\t{}", child.to_fen(), full);
            }
            board = child;
        }
    }
    eprintln!("incremental == refresh on {checked} positions");
    micro(&net);
    farzin::hka::profile(&net, &Board::startpos());
}

#[allow(dead_code)]
pub fn micro(net: &Network) {
    use std::time::Instant;
    let board = Board::from_fen("r1bqkb1r/pp2pppp/2n2n2/2pp4/2P5/P3PN2/1P1P1PPP/RNBQKB1R w KQkq - 0 1").unwrap();
    let moves = legal_moves(&board);
    let mut acc = Accumulators::new();
    acc.refresh(net, &board, 0);
    let n = 2_000_000;
    let t = Instant::now();
    let mut s = 0i64;
    for i in 0..n {
        let mv = moves[i % moves.len()];
        let child = board.make_move(mv);
        acc.push(net, &board, &child, mv, 1);
        s += 1;
    }
    let push = t.elapsed().as_nanos() as f64 / n as f64;
    let child = board.make_move(moves[0]);
    acc.push(net, &board, &child, moves[0], 1);
    let t = Instant::now();
    for _ in 0..n {
        s += i64::from(acc.evaluate(net, std::hint::black_box(&child), 1));
    }
    let ev = t.elapsed().as_nanos() as f64 / n as f64;
    eprintln!("push {push:.1} ns  evaluate {ev:.1} ns  ({s})");
}
