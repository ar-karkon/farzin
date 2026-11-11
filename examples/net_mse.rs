//! Accuracy of networks against labelled positions (teacher distillation
//! diagnostics).
//!
//! cargo run --release --example net_mse -- DATA NET [NET...]
//!
//! For each net prints the mean absolute error (cp) and the sigmoid-space
//! MSE (scale 400) between its static eval and the stored labels.
use farzin::board::Board;
use farzin::datagen::{unpack, RECORD_SIZE};
use farzin::evalnet::EvalNet;
use farzin::types::*;

#[path = "common/board_util.rs"]
mod board_util;
use board_util::board_from;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let data = std::fs::read(&args[1]).expect("data");
    let limit: usize = std::env::var("LIMIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200_000);
    let recs: Vec<&[u8]> = data
        .as_chunks::<RECORD_SIZE>()
        .0
        .iter()
        .take(limit)
        .map(|r| &r[..])
        .collect();
    let boards: Vec<(Board, i32)> = recs
        .iter()
        .map(|r| {
            let u = unpack(r);
            let s = i32::from(u.score);
            (board_from(r), if u.stm == WHITE { s } else { -s })
        })
        .collect();
    let sig = |x: f64| 1.0 / (1.0 + (-x / 400.0).exp());
    for path in &args[2..] {
        let net = EvalNet::load(path).expect("net");
        let (mut mae, mut mse) = (0.0f64, 0.0f64);
        for (b, label) in &boards {
            let v = net.evaluate(b);
            mae += f64::from((v - label).abs());
            let d = sig(f64::from(v)) - sig(f64::from(*label));
            mse += d * d;
        }
        let n = boards.len() as f64;
        println!(
            "{path}: n {} mae {:.1} cp  mse {:.6}",
            boards.len(),
            mae / n,
            mse / n
        );
    }
}
