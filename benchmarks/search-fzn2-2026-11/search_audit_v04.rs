//! Search diagnostics using the actual node count, including an interrupted iteration.
//! Usage: search_audit NETWORK POSITIONS nodes|depth LIMIT REPEATS
//! Output is TSV. Each position starts with cleared TT and histories; one thread.
use farzin::board::Board;
use farzin::evalnet::EvalNet;
use farzin::search::{search, Limits, Shared, Thread};
use farzin::tt::TranspositionTable;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 6 {
        eprintln!("usage: search_audit NETWORK POSITIONS nodes|depth LIMIT REPEATS");
        std::process::exit(2);
    }
    let net = Arc::new(EvalNet::load(&args[1]).expect("load requested network"));
    assert!(
        matches!(&*net, EvalNet::Fzn2(_)),
        "this audit requires FZN2"
    );
    let input = std::fs::read_to_string(&args[2]).expect("read positions");
    let positions: Vec<_> = input
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.starts_with('#'))
        .collect();
    let limit: u64 = args[4].parse().expect("positive limit");
    let repeats: usize = args[5].parse().expect("positive repeats");
    assert!(limit > 0 && repeats > 0);
    let limits = match args[3].as_str() {
        "nodes" => Limits {
            nodes: Some(limit),
            ..Limits::default()
        },
        "depth" => {
            assert!(limit <= 124);
            Limits {
                depth: Some(limit as i32),
                ..Limits::default()
            }
        }
        _ => panic!("limit mode must be nodes or depth"),
    };
    let mut shared = Shared {
        tt: TranspositionTable::new(16),
        stop: AtomicBool::new(false),
        nodes: AtomicU64::new(0),
    };
    let mut threads = vec![Thread::new(0)];
    threads[0].net = Some(net);
    threads[0].silent = true;
    println!("repeat\tposition\tnodes\tdepth\tscore\tbestmove\telapsed_ns\tpawn_corr\twhite_corr\tblack_corr\tpv");
    for repeat in 0..repeats {
        for (index, fen) in positions.iter().enumerate() {
            let board = if *fen == "startpos" {
                Board::startpos()
            } else {
                Board::from_fen(fen.strip_prefix("fen ").unwrap_or(fen)).expect("legal FEN")
            };
            shared.tt.clear();
            threads[0].histories.clear();
            let start = Instant::now();
            let result = search(&shared, &mut threads, &board, &[], limits);
            let elapsed = start.elapsed().as_nanos();
            let mut check = board;
            for &mv in &result.pv {
                assert!(
                    check.is_pseudo_legal(mv) && check.is_legal(mv),
                    "illegal PV at position {index}"
                );
                check = check.make_move(mv);
            }
            let h = &threads[0].histories;
            let size = h.pawn_corr[board.stm].len();
            let pawn = h.pawn_corr[board.stm][board.pawn_hash as usize % size];
            let white = h.non_pawn_corr[board.stm][0][board.non_pawn_hash[0] as usize % size];
            let black = h.non_pawn_corr[board.stm][1][board.non_pawn_hash[1] as usize % size];
            let pv = result
                .pv
                .iter()
                .map(|m| m.to_uci())
                .collect::<Vec<_>>()
                .join(" ");
            println!(
                "{repeat}\t{index}\t{}\t{}\t{}\t{}\t{elapsed}\t{pawn}\t{white}\t{black}\t{pv}",
                result.nodes,
                result.depth,
                result.score,
                result.best.to_uci()
            );
        }
    }
}
