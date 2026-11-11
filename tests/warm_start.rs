//! `farzin-train2 init=...` must start exactly at the old network, also
//! when the student grows (more king buckets, wider hidden layer).
use farzin::board::Board;
use farzin::datagen::pack;
use farzin::fzn2::Network;
use std::process::Command;

const FENS: &[&str] = &[
    "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    "r1bq1rk1/pp2bppp/2n1pn2/3p4/2PP4/2N1PN2/PP3PPP/R2QKB1R w KQ - 0 9",
    "8/5k2/3p4/1p1Pp2p/pP2Pp1P/P4P1K/8/8 b - - 99 50",
    "6k1/5ppp/8/8/8/8/5PPP/3R2K1 w - - 0 1",
    "2kr3r/ppp2ppp/2n5/4q3/4P3/2N5/PPP2PPP/R2QK2R b KQ - 0 14",
    "4k3/8/8/8/8/8/8/R3K2R w KQ - 0 1",
];

fn grow(dir: &std::path::Path, kb: &str, hidden: usize) -> Network {
    let data = dir.join("pos.data");
    let mut bytes = Vec::new();
    for fen in FENS {
        bytes.extend_from_slice(&pack(&Board::from_fen(fen).unwrap(), 0, 1));
    }
    std::fs::write(&data, bytes).unwrap();
    let out = dir.join(format!("grown-{kb}-{hidden}.nnue"));
    let status = Command::new(env!("CARGO_BIN_EXE_farzin-train2"))
        .arg(format!("data={}", data.display()))
        .arg(concat!(
            "init=",
            env!("CARGO_MANIFEST_DIR"),
            "/nets/fzn2-a1.nnue"
        ))
        .arg(format!("kb={kb}"))
        .arg(format!("hidden={hidden}"))
        .arg("epochs=0")
        .arg(format!("out={}", out.display()))
        .status()
        .unwrap();
    assert!(status.success());
    Network::load(out.to_str().unwrap()).unwrap()
}

#[test]
fn warm_start_growth_preserves_evaluation() {
    let dir = std::env::temp_dir().join(format!("farzin-warm-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let old = Network::load(concat!(env!("CARGO_MANIFEST_DIR"), "/nets/fzn2-a1.nnue")).unwrap();
    for (kb, hidden) in [
        ("4m", old.hidden),
        ("16m", old.hidden),
        ("4m", old.hidden + 256),
    ] {
        let new = grow(&dir, kb, hidden);
        assert_eq!(new.hidden, hidden);
        for fen in FENS {
            let b = Board::from_fen(fen).unwrap();
            assert_eq!(new.evaluate(&b), old.evaluate(&b), "{kb} x {hidden}: {fen}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
