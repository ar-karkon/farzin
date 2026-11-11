//! FZN2: Farzin's own king-bucketed NNUE.
//!
//! Architecture: `(KB x 768 -> H) x 2 -> 1 x OB`, SCReLU, material output
//! buckets. Each perspective's 768 piece-square inputs are selected by a
//! *king bucket* of that perspective's own king, and the board is mirrored
//! horizontally when that king stands on files e-h, so one set of weights
//! covers both wings. With `KB = 1` and no mirroring this is exactly FZN1.
//!
//! Inference keeps one accumulator per ply (eager incremental updates) and a
//! refresh cache ("Finny table") per (perspective, bucket, mirror): a king
//! move that changes bucket or mirror rebuilds only the difference between
//! the cached and the current position.
//!
//! File layout (little endian): `"FZN2"`, version u32 (=1), hidden u32,
//! output buckets u32, king buckets u32, flags u32 (bit 0: mirror), 64-byte
//! bucket layout (indexed by the king square seen from the perspective,
//! after mirroring), feature weights `[KB*768][H]` i16, feature bias `[H]`
//! i16, output weights `[OB][2H]` i16, output bias `[OB]` i32.
use crate::board::Board;
use crate::types::*;

pub const QA: i32 = 255;
pub const QB: i32 = 64;
pub const SCALE: i32 = 400;
pub const MAX_HIDDEN: usize = 2048;
pub const MAGIC: &[u8; 4] = b"FZN2";
const HEADER: usize = 24 + 64;

/// Named king-bucket layouts. Index = king square from the perspective's
/// view (rank 1 = own back rank) after mirroring onto files a-d; entries
/// for files e-h are only used when mirroring is off.
#[allow(clippy::needless_range_loop)]
pub fn layout(name: &str) -> Option<([u8; 64], usize, bool)> {
    let mut t = [0u8; 64];
    let (n, mirror) = match name {
        "1" => (1, false),
        "1m" => (1, true),
        // Back rank by file pair, rank 2, ranks 3-4, ranks 5-8.
        "4m" => {
            for sq in 0..64 {
                let (r, f) = (sq / 8, (sq % 8).min(7 - sq % 8));
                t[sq] = match r {
                    0 => u8::from(f >= 2),
                    1 => 2,
                    _ => 3,
                };
            }
            (4, true)
        }
        // Per-square on ranks 1-2, file pairs on ranks 3-4, wide zones above.
        "16m" => {
            for sq in 0..64 {
                let (r, f) = (sq / 8, (sq % 8).min(7 - sq % 8));
                t[sq] = match r {
                    0 => f as u8,
                    1 => 4 + f as u8,
                    2 => 8 + u8::from(f >= 2),
                    3 => 10 + u8::from(f >= 2),
                    4 | 5 => 12 + u8::from(f >= 2),
                    _ => 14 + u8::from(f >= 2),
                };
            }
            (16, true)
        }
        // One bucket per square of the mirrored half board.
        "32m" => {
            for sq in 0..64 {
                let (r, f) = (sq / 8, (sq % 8).min(7 - sq % 8));
                t[sq] = (r * 4 + f) as u8;
            }
            (32, true)
        }
        _ => return None,
    };
    Some((t, n, mirror))
}

pub struct Network {
    pub hidden: usize,
    pub out_buckets: usize,
    pub king_buckets: usize,
    pub mirror: bool,
    pub layout: [u8; 64],
    /// `[KB * 768][hidden]`
    pub ft_weights: Vec<i16>,
    pub ft_bias: Vec<i16>,
    /// `[out_buckets][2 * hidden]`
    pub out_weights: Vec<i16>,
    pub out_bias: Vec<i32>,
}

/// Bucket/mirror key of perspective `p` with its king on `ksq`.
#[inline(always)]
pub fn king_key(layout: &[u8; 64], mirror: bool, p: usize, ksq: Square) -> (usize, bool) {
    let rel = if p == WHITE { ksq } else { ksq ^ 56 };
    let m = mirror && rel % 8 >= 4;
    let idx = if m { rel ^ 7 } else { rel };
    (layout[idx] as usize, m)
}

/// Input index for piece `code` on `sq`, seen from perspective `p` whose
/// king key is `(bucket, mirrored)`.
#[inline(always)]
pub fn feature(p: usize, bucket: usize, mirrored: bool, code: usize, sq: Square) -> usize {
    let color = code / 6;
    let piece = code % 6;
    let rel_color = usize::from(color != p);
    let mut rel_sq = if p == WHITE { sq } else { sq ^ 56 };
    if mirrored {
        rel_sq ^= 7;
    }
    bucket * 768 + rel_color * 384 + piece * 64 + rel_sq
}

#[inline(always)]
pub fn output_bucket(pieces: usize, buckets: usize) -> usize {
    ((pieces - 2) * buckets / 31).min(buckets - 1)
}

impl Network {
    pub fn is_fzn2(bytes: &[u8]) -> bool {
        bytes.len() >= 4 && &bytes[..4] == MAGIC
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Network, String> {
        if bytes.len() < HEADER || &bytes[..4] != MAGIC {
            return Err("not a Farzin FZN2 network".into());
        }
        let u32_at = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap()) as usize;
        if u32_at(4) != 1 {
            return Err(format!("unsupported FZN2 version {}", u32_at(4)));
        }
        let hidden = u32_at(8);
        let out_buckets = u32_at(12);
        let king_buckets = u32_at(16);
        let mirror = u32_at(20) & 1 != 0;
        if hidden == 0
            || hidden > MAX_HIDDEN
            || hidden % 32 != 0
            || out_buckets == 0
            || out_buckets > 16
            || king_buckets == 0
            || king_buckets > 64
        {
            return Err(format!(
                "unsupported FZN2 shape {king_buckets}x768x{hidden}x{out_buckets}"
            ));
        }
        let mut layout = [0u8; 64];
        layout.copy_from_slice(&bytes[24..88]);
        if layout.iter().any(|&b| b as usize >= king_buckets) {
            return Err("FZN2 king bucket layout out of range".into());
        }
        let inputs = king_buckets * 768;
        let expected =
            HEADER + 2 * (inputs * hidden + hidden + out_buckets * 2 * hidden) + 4 * out_buckets;
        if bytes.len() != expected {
            return Err(format!("FZN2 size {} != expected {expected}", bytes.len()));
        }
        let mut offset = HEADER;
        let mut read_i16 = |n: usize| {
            let v: Vec<i16> = bytes[offset..offset + 2 * n]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| i16::from_le_bytes(*c))
                .collect();
            offset += 2 * n;
            v
        };
        let ft_weights = read_i16(inputs * hidden);
        let ft_bias = read_i16(hidden);
        let out_weights = read_i16(out_buckets * 2 * hidden);
        let out_bias = bytes[offset..offset + 4 * out_buckets]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| i32::from_le_bytes(*c))
            .collect();
        Ok(Network {
            hidden,
            out_buckets,
            king_buckets,
            mirror,
            layout,
            ft_weights,
            ft_bias,
            out_weights,
            out_bias,
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER + 2 * self.ft_weights.len());
        out.extend_from_slice(MAGIC);
        for v in [
            1u32,
            self.hidden as u32,
            self.out_buckets as u32,
            self.king_buckets as u32,
            u32::from(self.mirror),
        ] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&self.layout);
        for v in self
            .ft_weights
            .iter()
            .chain(&self.ft_bias)
            .chain(&self.out_weights)
        {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for v in &self.out_bias {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    }

    pub fn load(path: &str) -> Result<Network, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
        Network::from_bytes(&bytes)
    }

    #[inline(always)]
    fn row(&self, feature: usize) -> &[i16] {
        &self.ft_weights[feature * self.hidden..(feature + 1) * self.hidden]
    }

    #[inline(always)]
    fn key(&self, p: usize, board: &Board) -> (usize, bool) {
        king_key(&self.layout, self.mirror, p, board.king_sq(p))
    }

    /// Full (non-incremental) evaluation, for tests and tools.
    pub fn evaluate(&self, board: &Board) -> i32 {
        let h = self.hidden;
        let mut acc = [vec![0i16; h], vec![0i16; h]];
        for p in [WHITE, BLACK] {
            self.fill(&mut acc[p], p, board);
        }
        self.output(&acc[WHITE], &acc[BLACK], board)
    }

    fn fill(&self, acc: &mut [i16], p: usize, board: &Board) {
        acc.copy_from_slice(&self.ft_bias);
        let (b, m) = self.key(p, board);
        let mut occ = board.occupied();
        while occ != 0 {
            let sq = pop_lsb(&mut occ);
            let code = board.mailbox[sq] as usize;
            add_row(acc, self.row(feature(p, b, m, code, sq)));
        }
    }

    #[inline]
    fn output(&self, white: &[i16], black: &[i16], board: &Board) -> i32 {
        let h = self.hidden;
        let bucket = output_bucket(board.occupied().count_ones() as usize, self.out_buckets);
        let w = &self.out_weights[bucket * 2 * h..(bucket + 1) * 2 * h];
        let (us, them) = if board.stm == WHITE {
            (white, black)
        } else {
            (black, white)
        };
        let sum = screlu_dot(us, &w[..h]) + screlu_dot(them, &w[h..]);
        let raw = sum / QA + self.out_bias[bucket];
        raw * SCALE / (QA * QB)
    }
}

#[inline(always)]
fn add_row(acc: &mut [i16], row: &[i16]) {
    for (a, w) in acc.iter_mut().zip(row) {
        *a = a.wrapping_add(*w);
    }
}

#[inline(always)]
fn sub_row(acc: &mut [i16], row: &[i16]) {
    for (a, w) in acc.iter_mut().zip(row) {
        *a = a.wrapping_sub(*w);
    }
}

/// `sum(clamp(a, 0, QA)^2 * w)`, computed as `(c * w) * c` so the first
/// product stays in i16 (|w| <= 128) and the second maps to a
/// multiply-add of i16 pairs into i32 lanes.
#[inline]
fn screlu_dot(acc: &[i16], weights: &[i16]) -> i32 {
    #[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
    {
        // SAFETY: AVX2 is enabled at compile time; slices have equal length,
        // a multiple of 32, and unaligned loads are used.
        unsafe { screlu_dot_avx2(acc, weights) }
    }
    #[cfg(not(all(target_arch = "x86_64", target_feature = "avx2")))]
    {
        screlu_dot_scalar(acc, weights)
    }
}

#[cfg_attr(all(target_arch = "x86_64", target_feature = "avx2"), allow(dead_code))]
#[inline]
fn screlu_dot_scalar(acc: &[i16], weights: &[i16]) -> i32 {
    let mut sums = [0i32; 32];
    for (a, w) in acc
        .as_chunks::<32>()
        .0
        .iter()
        .zip(weights.as_chunks::<32>().0)
    {
        for i in 0..32 {
            let c = a[i].clamp(0, QA as i16);
            let cw = c.wrapping_mul(w[i]);
            sums[i] += i32::from(cw) * i32::from(c);
        }
    }
    sums.iter().sum()
}

#[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
#[inline]
unsafe fn screlu_dot_avx2(acc: &[i16], weights: &[i16]) -> i32 {
    use std::arch::x86_64::*;
    let zero = _mm256_setzero_si256();
    let qa = _mm256_set1_epi16(QA as i16);
    let mut s0 = _mm256_setzero_si256();
    let mut s1 = _mm256_setzero_si256();
    let n = acc.len();
    let (a, w) = (acc.as_ptr(), weights.as_ptr());
    let mut i = 0;
    while i < n {
        let x0 = _mm256_loadu_si256(a.add(i) as *const __m256i);
        let x1 = _mm256_loadu_si256(a.add(i + 16) as *const __m256i);
        let w0 = _mm256_loadu_si256(w.add(i) as *const __m256i);
        let w1 = _mm256_loadu_si256(w.add(i + 16) as *const __m256i);
        let c0 = _mm256_min_epi16(_mm256_max_epi16(x0, zero), qa);
        let c1 = _mm256_min_epi16(_mm256_max_epi16(x1, zero), qa);
        s0 = _mm256_add_epi32(s0, _mm256_madd_epi16(_mm256_mullo_epi16(c0, w0), c0));
        s1 = _mm256_add_epi32(s1, _mm256_madd_epi16(_mm256_mullo_epi16(c1, w1), c1));
        i += 32;
    }
    let s = _mm256_add_epi32(s0, s1);
    let lo = _mm256_castsi256_si128(s);
    let hi = _mm256_extracti128_si256::<1>(s);
    let x = _mm_add_epi32(lo, hi);
    let x = _mm_add_epi32(x, _mm_shuffle_epi32::<0b0100_1110>(x));
    let x = _mm_add_epi32(x, _mm_shuffle_epi32::<0b1011_0001>(x));
    _mm_cvtsi128_si32(x)
}

#[inline]
fn apply(dst: &mut [i16], src: &[i16], adds: &[&[i16]], subs: &[&[i16]]) {
    match (adds.len(), subs.len()) {
        (1, 1) => {
            let (a, s) = (adds[0], subs[0]);
            for i in 0..dst.len() {
                dst[i] = src[i].wrapping_add(a[i]).wrapping_sub(s[i]);
            }
        }
        (1, 2) => {
            let (a, s1, s2) = (adds[0], subs[0], subs[1]);
            for i in 0..dst.len() {
                dst[i] = src[i]
                    .wrapping_add(a[i])
                    .wrapping_sub(s1[i])
                    .wrapping_sub(s2[i]);
            }
        }
        (2, 2) => {
            let (a1, a2, s1, s2) = (adds[0], adds[1], subs[0], subs[1]);
            for i in 0..dst.len() {
                dst[i] = src[i]
                    .wrapping_add(a1[i])
                    .wrapping_add(a2[i])
                    .wrapping_sub(s1[i])
                    .wrapping_sub(s2[i]);
            }
        }
        _ => {
            dst.copy_from_slice(src);
            for a in adds {
                add_row(dst, a);
            }
            for s in subs {
                sub_row(dst, s);
            }
        }
    }
}

struct CacheEntry {
    acc: Vec<i16>,
    bbs: [Bitboard; 12],
}

/// Per-ply accumulator stack and king-bucket refresh cache.
pub struct Accumulators {
    /// `stack[ply][perspective]`
    stack: Vec<[Vec<i16>; 2]>,
    /// `cache[perspective][bucket * 2 + mirrored]`
    cache: [Vec<CacheEntry>; 2],
    hidden: usize,
    king_buckets: usize,
}

impl Default for Accumulators {
    fn default() -> Self {
        Self::new()
    }
}

fn board_bbs(board: &Board) -> [Bitboard; 12] {
    let mut out = [0; 12];
    for (c, color) in board.colors.iter().enumerate() {
        for (pc, bb) in board.pieces.iter().enumerate() {
            out[c * 6 + pc] = bb & color;
        }
    }
    out
}

impl Accumulators {
    pub fn new() -> Self {
        Accumulators {
            stack: Vec::new(),
            cache: [Vec::new(), Vec::new()],
            hidden: 0,
            king_buckets: 0,
        }
    }

    fn ensure(&mut self, net: &Network) {
        if self.hidden != net.hidden
            || self.king_buckets != net.king_buckets
            || self.stack.is_empty()
        {
            self.hidden = net.hidden;
            self.king_buckets = net.king_buckets;
            self.stack = (0..crate::search::MAX_PLY + 8)
                .map(|_| [vec![0; net.hidden], vec![0; net.hidden]])
                .collect();
        }
        // Cache contents depend on the weights, so always rebuild it on a
        // root refresh (cheap: entries start empty and fill lazily).
        for p in 0..2 {
            self.cache[p] = (0..2 * net.king_buckets)
                .map(|_| CacheEntry {
                    acc: net.ft_bias.clone(),
                    bbs: [0; 12],
                })
                .collect();
        }
    }

    pub fn refresh(&mut self, net: &Network, board: &Board, ply: usize) {
        self.ensure(net);
        for p in [WHITE, BLACK] {
            self.refresh_side(net, board, ply, p);
        }
    }

    /// Rebuild perspective `p` at `ply` from the refresh cache.
    #[allow(clippy::needless_range_loop)]
    fn refresh_side(&mut self, net: &Network, board: &Board, ply: usize, p: usize) {
        let (b, m) = net.key(p, board);
        let entry = &mut self.cache[p][b * 2 + usize::from(m)];
        let now = board_bbs(board);
        for code in 0..12 {
            let mut add = now[code] & !entry.bbs[code];
            let mut sub = entry.bbs[code] & !now[code];
            while add != 0 {
                let sq = pop_lsb(&mut add);
                add_row(&mut entry.acc, net.row(feature(p, b, m, code, sq)));
            }
            while sub != 0 {
                let sq = pop_lsb(&mut sub);
                sub_row(&mut entry.acc, net.row(feature(p, b, m, code, sq)));
            }
        }
        entry.bbs = now;
        self.stack[ply][p].copy_from_slice(&entry.acc);
    }

    /// Accumulator for `child` (= `parent` after `mv`) at `ply`.
    pub fn push(&mut self, net: &Network, parent: &Board, child: &Board, mv: Move, ply: usize) {
        let us = parent.stm;
        let from = mv.from();
        let to = mv.to();
        let code = parent.mailbox[from] as usize;
        let placed = if mv.is_promotion() {
            us * 6 + mv.promotion()
        } else {
            code
        };
        for p in [WHITE, BLACK] {
            if code == us * 6 + KING && p == us {
                let old = net.key(p, parent);
                let new = net.key(p, child);
                if old != new {
                    self.refresh_side(net, child, ply, p);
                    continue;
                }
            }
            let (b, m) = net.key(p, child);
            let f = |c: usize, s: Square| net.row(feature(p, b, m, c, s));
            let add1 = f(placed, to);
            let sub1 = f(code, from);
            let (prev, rest) = self.stack.split_at_mut(ply);
            let src = &prev[ply - 1][p];
            let dst = &mut rest[0][p];
            if mv.is_castle() {
                let (rf, rt) = if mv.flag() == KING_CASTLE {
                    (to + 1, to - 1)
                } else {
                    (to - 2, to + 1)
                };
                let rook = us * 6 + ROOK;
                apply(dst, src, &[add1, f(rook, rt)], &[sub1, f(rook, rf)]);
            } else if mv.is_capture() {
                let (cap_sq, cap_code) = if mv.flag() == EN_PASSANT {
                    (to ^ 8, (us ^ 1) * 6 + PAWN)
                } else {
                    (to, parent.mailbox[to] as usize)
                };
                apply(dst, src, &[add1], &[sub1, f(cap_code, cap_sq)]);
            } else {
                apply(dst, src, &[add1], &[sub1]);
            }
        }
    }

    pub fn push_null(&mut self, ply: usize) {
        let (prev, rest) = self.stack.split_at_mut(ply);
        for p in [WHITE, BLACK] {
            rest[0][p].copy_from_slice(&prev[ply - 1][p]);
        }
    }

    pub fn evaluate(&self, net: &Network, board: &Board, ply: usize) -> i32 {
        let [w, b] = &self.stack[ply];
        net.output(w, b, board)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::movegen::legal_moves;

    pub fn random_net(hidden: usize, out_buckets: usize, layout_name: &str) -> Network {
        let (layout, kb, mirror) = layout(layout_name).unwrap();
        let mut s = 0x1234_5678u64;
        let mut next = move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            ((s >> 40) % 200) as i16 - 100
        };
        Network {
            hidden,
            out_buckets,
            king_buckets: kb,
            mirror,
            layout,
            ft_weights: (0..kb * 768 * hidden).map(|_| next()).collect(),
            ft_bias: (0..hidden).map(|_| next()).collect(),
            out_weights: (0..out_buckets * 2 * hidden).map(|_| next() / 2).collect(),
            out_bias: (0..out_buckets).map(|_| i32::from(next()) * 100).collect(),
        }
    }

    #[test]
    fn simd_matches_scalar() {
        let mut x = 99u32;
        let mut r = || {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
            ((x >> 16) % 700) as i16 - 200
        };
        let a: Vec<i16> = (0..256).map(|_| r()).collect();
        let w: Vec<i16> = (0..256).map(|_| r() % 128).collect();
        assert_eq!(screlu_dot(&a, &w), screlu_dot_scalar(&a, &w));
    }

    #[test]
    fn layouts_are_in_range() {
        for name in ["1", "1m", "4m", "16m", "32m"] {
            let (t, n, _) = layout(name).unwrap();
            assert!(t.iter().all(|&b| (b as usize) < n), "{name}");
            for b in 0..n {
                assert!(
                    t.iter().any(|&x| x as usize == b),
                    "{name} bucket {b} unused"
                );
            }
        }
    }

    #[test]
    fn mirror_symmetry() {
        // With mirroring, a position and its horizontal mirror evaluate
        // identically (castling rights cleared so the mirror is legal).
        let net = random_net(32, 8, "16m");
        let a = Board::from_fen("4k3/pp3p2/2n5/3qp3/8/2N2B2/PP3PPP/2R2K2 w - - 0 1").unwrap();
        let b = Board::from_fen("3k4/2p3pp/5n2/3pq3/8/2B2N2/PPP3PP/2K2R2 w - - 0 1").unwrap();
        assert_eq!(net.evaluate(&a), net.evaluate(&b));
    }

    #[test]
    fn incremental_updates_match_full_evaluation() {
        for layout_name in ["1", "16m", "32m"] {
            let net = random_net(64, 8, layout_name);
            let reloaded = Network::from_bytes(&net.to_bytes()).unwrap();
            assert_eq!(reloaded.ft_weights, net.ft_weights);
            assert_eq!(reloaded.layout, net.layout);
            assert_eq!(reloaded.out_bias, net.out_bias);
            let mut acc = Accumulators::new();
            for fen in [
                "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
                "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
                "rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPP1PPP/RNBQKBNR w KQkq f6 0 3",
                "8/2k5/8/3K4/8/8/5P2/8 w - - 0 1",
            ] {
                let mut board = Board::from_fen(fen).unwrap();
                acc.refresh(&net, &board, 0);
                let mut seed = 7usize;
                for ply in 1..60 {
                    let moves = legal_moves(&board);
                    if moves.is_empty() {
                        break;
                    }
                    let special: Vec<_> = moves
                        .iter()
                        .copied()
                        .filter(|m| {
                            m.is_noisy()
                                || m.is_castle()
                                || board.mailbox[m.from()] as usize % 6 == KING
                        })
                        .collect();
                    seed = seed
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
                    let pool = if !special.is_empty() && seed.is_multiple_of(2) {
                        &special
                    } else {
                        &moves
                    };
                    let mv = pool[(seed >> 33) % pool.len()];
                    let child = board.make_move(mv);
                    acc.push(&net, &board, &child, mv, ply);
                    assert_eq!(
                        acc.evaluate(&net, &child, ply),
                        net.evaluate(&child),
                        "{layout_name} {fen} {}",
                        mv.to_uci()
                    );
                    board = child;
                }
            }
        }
    }
}
