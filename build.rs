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

    // PEXT slider tables, only when the target has BMI2.
    let features = env::var("CARGO_CFG_TARGET_FEATURE").unwrap_or_default();
    if features.split(',').any(|f| f == "bmi2") {
        fs::write(Path::new(&out_dir).join("pext.rs"), pext_tables()).unwrap();
    }

    embed_network(&out_dir);
}

/// Software PEXT (build time only).
fn pext(src: u64, mut mask: u64) -> u64 {
    let mut out = 0;
    let mut bit = 1u64;
    while mask != 0 {
        let low = 1u64 << mask.trailing_zeros();
        if src & low != 0 {
            out |= bit;
        }
        bit <<= 1;
        mask &= mask - 1;
    }
    out
}

fn pext_tables() -> String {
    let mut out = String::new();
    for (name, dirs) in [("ROOK", &ROOK_DIRS), ("BISHOP", &BISHOP_DIRS)] {
        let mut entries = Vec::new();
        let mut table: Vec<u64> = Vec::new();
        for sq in 0..64 {
            let mask = relevant_mask(sq, dirs);
            let offset = table.len();
            let size = 1usize << mask.count_ones();
            table.resize(offset + size, 0);
            let mut subset = 0u64;
            loop {
                table[offset + pext(subset, mask) as usize] = sliding(sq, subset, dirs);
                subset = subset.wrapping_sub(mask) & mask;
                if subset == 0 {
                    break;
                }
            }
            entries.push((mask, offset));
        }
        let _ = writeln!(out, "pub(crate) const {name}_PEXT: [PextEntry; 64] = [");
        for (mask, offset) in entries {
            let _ = writeln!(
                out,
                "PextEntry {{ mask: 0x{mask:016x}, offset: {offset} }},"
            );
        }
        let _ = writeln!(out, "];");
        let _ = writeln!(
            out,
            "pub(crate) static {name}_PEXT_TABLE: [u64; {}] = [",
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
    out
}

/// Embedded network.
///
/// * `ownnet` feature (the CCRL / competition edition): Farzin's own FZN2
///   network `nets/fzn2-a1.nnue`. Any other network format is refused at
///   build time, so this edition can never carry third-party weights.
/// * default (research edition): `$FARZIN_EVALFILE`, else
///   `nets/default.nnue`, else none (classical evaluation).
fn embed_network(out_dir: &str) {
    println!("cargo:rerun-if-env-changed=FARZIN_EVALFILE");
    let ownnet = env::var_os("CARGO_FEATURE_OWNNET").is_some();
    let source = if ownnet {
        "nets/fzn2-a1.nnue".to_string()
    } else {
        env::var("FARZIN_EVALFILE").unwrap_or_else(|_| "nets/default.nnue".to_string())
    };
    println!("cargo:rerun-if-changed={source}");
    let bytes = fs::read(&source).unwrap_or_default();
    if ownnet {
        assert!(
            bytes.starts_with(b"FZN2"),
            "ownnet edition: {source} is missing or not an FZN2 network"
        );
        assert_eq!(
            sha256_hex(&bytes),
            OWNNET_SHA256,
            "ownnet edition: {source} does not match the released FZN2 network"
        );
    }
    let digest = if bytes.is_empty() {
        String::new()
    } else {
        sha256_hex(&bytes)
    };
    let name = Path::new(&source)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    fs::write(Path::new(out_dir).join("default.nnue"), &bytes).unwrap();
    fs::write(
        Path::new(out_dir).join("embedded_net.rs"),
        format!(
            "pub const EMBEDDED_NET_NAME: &str = {name:?};\n\
             pub const EMBEDDED_NET_SHA256: &str = {digest:?};\n"
        ),
    )
    .unwrap();
}

/// SHA-256 of the released own network `nets/fzn2-a1.nnue`.
const OWNNET_SHA256: &str = "f7bb0b7ef9857c3a4f24b39f5152e381e978d76d0c24817b0bebbefb1b6e220c";

/// Minimal SHA-256 (FIPS 180-4) so the build has no extra dependencies.
#[allow(clippy::needless_range_loop)]
fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for block in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(block[i * 4..i * 4 + 4].try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7]
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v[7] = v[6];
            v[6] = v[5];
            v[5] = v[4];
            v[4] = v[3].wrapping_add(t1);
            v[3] = v[2];
            v[2] = v[1];
            v[1] = v[0];
            v[0] = t1.wrapping_add(t2);
        }
        for i in 0..8 {
            h[i] = h[i].wrapping_add(v[i]);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}
