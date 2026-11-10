//! Generates magic-bitboard tables for the engine core at build time so the
//! hot attack lookups are plain static array reads with no runtime init.
use std::fmt::Write as _;
use std::{env, fs, path::Path};

const ROOK_DIRS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
const BISHOP_DIRS: [(i32, i32); 4] = [(1, 1), (1, -1), (-1, 1), (-1, -1)];

fn on_board(r: i32, f: i32) -> bool {
    (0..8).contains(&r) && (0..8).contains(&f)
}

fn sliding(sq: usize, occ: u64, dirs: &[(i32, i32)]) -> u64 {
    let (r0, f0) = ((sq / 8) as i32, (sq % 8) as i32);
    let mut attacks = 0;
    for &(dr, df) in dirs {
        let (mut r, mut f) = (r0 + dr, f0 + df);
        while on_board(r, f) {
            let bit = 1u64 << (r * 8 + f);
            attacks |= bit;
            if occ & bit != 0 {
                break;
            }
            r += dr;
            f += df;
        }
    }
    attacks
}

fn relevant_mask(sq: usize, dirs: &[(i32, i32)]) -> u64 {
    let (r0, f0) = ((sq / 8) as i32, (sq % 8) as i32);
    let mut mask = 0;
    for &(dr, df) in dirs {
        let (mut r, mut f) = (r0 + dr, f0 + df);
        while on_board(r + dr, f + df) {
            mask |= 1u64 << (r * 8 + f);
            r += dr;
            f += df;
        }
    }
    mask
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn sparse(&mut self) -> u64 {
        self.next() & self.next() & self.next()
    }
}

struct Magic {
    mask: u64,
    magic: u64,
    shift: u32,
    offset: usize,
}

fn find_magics(dirs: &[(i32, i32)], rng: &mut Rng) -> (Vec<Magic>, Vec<u64>) {
    let mut magics = Vec::new();
    let mut table = Vec::new();
    for sq in 0..64 {
        let mask = relevant_mask(sq, dirs);
        let bits = mask.count_ones();
        let size = 1usize << bits;
        let mut occs = Vec::with_capacity(size);
        let mut atts = Vec::with_capacity(size);
        let mut subset = 0u64;
        loop {
            occs.push(subset);
            atts.push(sliding(sq, subset, dirs));
            subset = subset.wrapping_sub(mask) & mask;
            if subset == 0 {
                break;
            }
        }
        let shift = 64 - bits;
        let mut used = vec![0u64; size];
        let mut epoch = vec![0u32; size];
        let mut attempt = 0u32;
        let magic = loop {
            let candidate = rng.sparse();
            if (mask.wrapping_mul(candidate) >> 56).count_ones() < 6 {
                continue;
            }
            attempt += 1;
            let mut ok = true;
            for i in 0..occs.len() {
                let idx = (occs[i].wrapping_mul(candidate) >> shift) as usize;
                if epoch[idx] != attempt {
                    epoch[idx] = attempt;
                    used[idx] = atts[i];
                } else if used[idx] != atts[i] {
                    ok = false;
                    break;
                }
            }
            if ok {
                break candidate;
            }
        };
        let offset = table.len();
        table.resize(offset + size, 0);
        for i in 0..occs.len() {
            let idx = (occs[i].wrapping_mul(magic) >> shift) as usize;
            table[offset + idx] = atts[i];
        }
        magics.push(Magic {
            mask,
            magic,
            shift,
            offset,
        });
    }
    (magics, table)
}

fn emit(out: &mut String, name: &str, magics: &[Magic], table: &[u64]) {
    let _ = writeln!(out, "pub(crate) const {name}_MAGICS: [MagicEntry; 64] = [");
    for m in magics {
        let _ = writeln!(
            out,
            "MagicEntry {{ mask: 0x{:016x}, magic: 0x{:016x}, shift: {}, offset: {} }},",
            m.mask, m.magic, m.shift, m.offset
        );
    }
    let _ = writeln!(out, "];");
    let _ = writeln!(
        out,
        "pub(crate) static {name}_TABLE: [u64; {}] = [",
        table.len()
    );
    for chunk in table.chunks(8) {
        for v in chunk {
            let _ = write!(out, "0x{v:x},");
        }
        out.push('\n');
    }
    let _ = writeln!(out, "];");
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let (rook, rook_table) = find_magics(&ROOK_DIRS, &mut rng);
    let (bishop, bishop_table) = find_magics(&BISHOP_DIRS, &mut rng);
    let mut out = String::new();
    emit(&mut out, "ROOK", &rook, &rook_table);
    emit(&mut out, "BISHOP", &bishop, &bishop_table);
    let out_dir = env::var("OUT_DIR").unwrap();
    fs::write(Path::new(&out_dir).join("magics.rs"), out).unwrap();

    // Embedded default network: $FARZIN_EVALFILE, else nets/default.nnue,
    // else none (the engine then falls back to the classical evaluation).
    println!("cargo:rerun-if-env-changed=FARZIN_EVALFILE");
    println!("cargo:rerun-if-changed=nets/default.nnue");
    let source = env::var("FARZIN_EVALFILE").unwrap_or_else(|_| "nets/default.nnue".to_string());
    let bytes = fs::read(&source).unwrap_or_default();
    fs::write(Path::new(&out_dir).join("default.nnue"), bytes).unwrap();
}
