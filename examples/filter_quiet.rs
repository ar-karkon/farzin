//! Keep only quiet positions (not in check, no capture with SEE > 0) of a
//! Farzin 32-byte record file.
//!
//! cargo run --release --example filter_quiet -- IN OUT
use farzin::datagen::RECORD_SIZE;
use std::io::{BufWriter, Read, Write};

#[path = "common/board_util.rs"]
mod board_util;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut f = std::fs::File::open(&args[1]).expect("input");
    let mut out = BufWriter::new(std::fs::File::create(&args[2]).expect("output"));
    let (mut kept, mut total) = (0u64, 0u64);
    let mut buf = vec![0u8; RECORD_SIZE * 65536];
    loop {
        let n = f.read(&mut buf).unwrap();
        if n == 0 {
            break;
        }
        let mut n = n;
        while !n.is_multiple_of(RECORD_SIZE) {
            let m = f.read(&mut buf[n..]).unwrap();
            assert!(m > 0, "truncated file");
            n += m;
        }
        for rec in buf[..n].chunks_exact(RECORD_SIZE) {
            total += 1;
            let b = board_util::board_from(rec);
            if farzin::search::is_quiet(&b) {
                out.write_all(rec).unwrap();
                kept += 1;
            }
        }
    }
    eprintln!("kept {kept} of {total}");
}
