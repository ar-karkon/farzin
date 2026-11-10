//! Diagnostic: compare i32 and i64 output sums of a network over a data file.
use farzin::datagen::{unpack, RECORD_SIZE};
use farzin::net::{feature, Network, QA};
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let net = Network::load(&args[1]).unwrap();
    let data = std::fs::read(&args[2]).unwrap();
    let h = net.hidden;
    let (mut n, mut overflow, mut max_abs) = (0u64, 0u64, 0i64);
    for rec in data.chunks_exact(RECORD_SIZE).step_by(97).take(20000) {
        let r = unpack(rec);
        let mut acc = [net.ft_bias.clone(), net.ft_bias.clone()];
        for &(code, sq) in &r.pieces {
            for p in 0..2 {
                let f = feature(if p == 0 { r.stm } else { r.stm ^ 1 }, code, sq);
                for i in 0..h {
                    acc[p][i] = acc[p][i].wrapping_add(net.ft_weights[f * h + i]);
                }
            }
        }
        let b = ((r.pieces.len() - 2) * net.buckets / 31).min(net.buckets - 1);
        let w = &net.out_weights[b * 2 * h..(b + 1) * 2 * h];
        let (mut s32, mut s64) = (0i32, 0i64);
        for p in 0..2 {
            for i in 0..h {
                let v = i32::from(acc[p][i]).clamp(0, QA);
                let t = v * v * i32::from(w[p * h + i]);
                s32 = s32.wrapping_add(t);
                s64 += i64::from(t);
            }
        }
        n += 1;
        if i64::from(s32) != s64 {
            overflow += 1;
        }
        max_abs = max_abs.max(s64.abs());
    }
    println!(
        "positions {n} overflowed {overflow} max |sum| {max_abs} (i32 max {})",
        i32::MAX
    );
}
