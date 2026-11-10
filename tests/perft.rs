use farzin::board::Board;
use farzin::movegen::perft;

// Reference counts from the chessprogramming.org perft results page.
const CASES: &[(&str, &[u64])] = &[
    (
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        &[20, 400, 8902, 197_281, 4_865_609],
    ),
    (
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        &[48, 2039, 97_862, 4_085_603],
    ),
    (
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        &[14, 191, 2812, 43_238, 674_624],
    ),
    (
        "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
        &[6, 264, 9467, 422_333],
    ),
    (
        "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
        &[44, 1486, 62_379, 2_103_487],
    ),
    (
        "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
        &[46, 2079, 89_890, 3_894_594],
    ),
];

#[test]
fn perft_reference_positions() {
    for (fen, counts) in CASES {
        let board = Board::from_fen(fen).unwrap();
        for (depth, &expected) in counts.iter().enumerate() {
            if cfg!(debug_assertions) && expected > 500_000 {
                continue;
            }
            assert_eq!(
                perft(&board, depth as u32 + 1),
                expected,
                "{fen} depth {}",
                depth + 1
            );
        }
    }
}

#[test]
fn fen_round_trip_and_hash_consistency() {
    for (fen, _) in CASES {
        let board = Board::from_fen(fen).unwrap();
        assert_eq!(board.to_fen(), *fen);
        // Incremental hashes match a fresh parse after every legal move.
        for mv in farzin::movegen::legal_moves(&board) {
            let child = board.make_move(mv);
            let fresh = Board::from_fen(&child.to_fen()).unwrap();
            assert_eq!(child.hash, fresh.hash, "{fen} {}", mv.to_uci());
            assert_eq!(child.pawn_hash, fresh.pawn_hash);
            assert_eq!(child.non_pawn_hash, fresh.non_pawn_hash);
            assert_eq!(child.checkers, fresh.checkers);
            assert_eq!(child.pinned, fresh.pinned);
        }
    }
}
