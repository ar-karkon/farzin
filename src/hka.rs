//! Independent Rust inference for the `HalfKAv2_hm` NNUE family
//! (`(22528 -> L1)x2 -> 15(+1) -> 32 -> 1`, 8 PSQT buckets, 8 layer stacks).
//!
//! This reads network files in the binary layout published by the Stockfish
//! project for its 15.x generation (for example `nn-ad9b42354671.nnue`, which
//! was released under CC0 in <https://github.com/official-stockfish/networks>).
//! No Stockfish source code is used: the module is written from the format
//! description and was verified against the reference engine's raw output
//! (see `docs/v6-report.md` and `tests/hka_oracle.rs`).
//!
//! Layout of one network (all little-endian):
//! ```text
//! u32 version, u32 hash, u32 desc_len, desc bytes
//! u32 ft_hash, i16 bias[L1], i16 weight[22528][L1], i32 psqt[22528][8]
//! 8x { u32 hash,
//!      i32 b0[16], i8 w0[16][L1],
//!      i32 b1[32], i8 w1[32][32]   (30 inputs padded to 32),
//!      i32 b2[1],  i8 w2[1][32] }
//! ```
use crate::board::Board;
use crate::types::*;

pub const INPUTS: usize = 22528; // 32 king buckets x 11 x 64
pub const PSQT_BUCKETS: usize = 8;
pub const STACKS: usize = 8;
const FC0_OUT: usize = 16; // 15 + 1 forwarded output
const FC1_IN: usize = 32; // 30 padded
const FC1_OUT: usize = 32;
const OUTPUT_SCALE: i32 = 16;
const WEIGHT_SCALE_BITS: i32 = 6;

pub struct Stack {
    b0: [i32; FC0_OUT],
    /// `[L1 / 4][16][4]`: for input chunk `c`, outputs 0..16, inputs `4c..4c+4`.
    w0: Vec<i8>,
    b1: [i32; FC1_OUT],
    #[cfg_attr(all(target_arch = "x86_64", target_feature = "avx2"), allow(dead_code))]
    w1: [[i8; FC1_IN]; FC1_OUT],
    /// `w1` as `[FC1_IN / 4][32][4]` for broadcast multiply-add.
    w1s: [i8; FC1_IN * FC1_OUT],
    b2: i32,
    w2: [i8; FC1_OUT],
}

pub struct Network {
    pub l1: usize,
    pub description: String,
    ft_bias: Vec<i16>,
    /// `[INPUTS][L1]`
    ft_w: Vec<i16>,
    /// `[INPUTS][8]`
    psqt_w: Vec<i32>,
    stacks: Vec<Stack>,
}

#[inline(always)]
fn orient(persp: usize, ksq: Square) -> usize {
    (if persp == WHITE { 0 } else { 56 }) ^ (if file_of(ksq) < 4 { 7 } else { 0 })
}

#[inline(always)]
fn king_bucket(persp: usize, ksq: Square) -> usize {
    let r = rank_of(ksq);
    let rel = if persp == WHITE { r } else { 7 - r };
    let f = file_of(ksq);
    (7 - rel) * 4 + f.min(7 - f)
}

/// Feature index of mailbox piece `code` on `sq` as seen by `persp` whose king is on `ksq`.
#[inline(always)]
pub fn feature(persp: usize, ksq: Square, code: usize, sq: Square) -> usize {
    let color = code / 6;
    let piece = code % 6;
    let ps = if piece == KING {
        640
    } else {
        (2 * piece + usize::from(color != persp)) * 64
    };
    (sq ^ orient(persp, ksq)) + ps + king_bucket(persp, ksq) * 704
}

const LEB128_MAGIC: &[u8] = b"COMPRESSED_LEB128";
const STACK_FIXED: usize = 4 + 4 * FC0_OUT + 4 * FC1_OUT + FC1_OUT * FC1_IN + 4 + FC1_OUT;

struct Reader<'a> {
    b: &'a [u8],
    o: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        if self.o + n > self.b.len() {
            return Err("truncated network".into());
        }
        let s = &self.b[self.o..self.o + n];
        self.o += n;
        Ok(s)
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i16s(&mut self, n: usize) -> Result<Vec<i16>, String> {
        Ok(self
            .take(2 * n)?
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect())
    }
    fn i32s(&mut self, n: usize) -> Result<Vec<i32>, String> {
        Ok(self
            .take(4 * n)?
            .chunks_exact(4)
            .map(|c| i32::from_le_bytes(c.try_into().unwrap()))
            .collect())
    }
    /// One signed-LEB128 block: magic, u32 byte count, payload.
    fn leb128(&mut self) -> Result<Vec<i32>, String> {
        if self.take(LEB128_MAGIC.len())? != LEB128_MAGIC {
            return Err("missing LEB128 block".into());
        }
        let len = self.u32()? as usize;
        let data = self.take(len)?;
        let mut out = Vec::with_capacity(len);
        let (mut value, mut shift) = (0i64, 0u32);
        for &byte in data {
            value |= i64::from(byte & 0x7f) << shift;
            shift += 7;
            if byte & 0x80 == 0 {
                if shift < 64 && byte & 0x40 != 0 {
                    value |= -1i64 << shift;
                }
                out.push(value as i32);
                value = 0;
                shift = 0;
            } else if shift > 35 {
                return Err("bad LEB128 value".into());
            }
        }
        Ok(out)
    }
    fn i8s(&mut self, n: usize) -> Result<Vec<i8>, String> {
        Ok(self.take(n)?.iter().map(|&x| x as i8).collect())
    }
}

impl Network {
    pub fn is_hka(bytes: &[u8]) -> bool {
        bytes.len() > 12 && u32::from_le_bytes(bytes[..4].try_into().unwrap()) == 0x7AF3_2F20
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Network, String> {
        if !Self::is_hka(bytes) {
            return Err("not a HalfKAv2_hm network".into());
        }
        let mut r = Reader { b: bytes, o: 4 };
        let _hash = r.u32()?;
        let dlen = r.u32()? as usize;
        let description = String::from_utf8_lossy(r.take(dlen)?).into_owned();
        let header = r.o;
        let _ft_hash = r.u32()?;
        let (l1, ft_bias, ft_w, psqt_w) = if bytes[r.o..].starts_with(LEB128_MAGIC) {
            let ft_bias: Vec<i16> = r.leb128()?.into_iter().map(|v| v as i16).collect();
            let l1 = ft_bias.len();
            let ft_w: Vec<i16> = r.leb128()?.into_iter().map(|v| v as i16).collect();
            let psqt_w: Vec<i32> = r.leb128()?;
            if ft_w.len() != INPUTS * l1 || psqt_w.len() != INPUTS * PSQT_BUCKETS {
                return Err("inconsistent compressed feature transformer".into());
            }
            (l1, ft_bias, ft_w, psqt_w)
        } else {
            // Infer L1 from the total size.
            let fixed = 4 + 4 * PSQT_BUCKETS * INPUTS + STACKS * STACK_FIXED;
            let per_l1 = 2 + 2 * INPUTS + STACKS * FC0_OUT;
            let rest = bytes.len() - header;
            if rest < fixed || !(rest - fixed).is_multiple_of(per_l1) {
                return Err(format!(
                    "unsupported HalfKAv2_hm layout ({} bytes)",
                    bytes.len()
                ));
            }
            let l1 = (rest - fixed) / per_l1;
            (
                l1,
                r.i16s(l1)?,
                r.i16s(INPUTS * l1)?,
                r.i32s(INPUTS * PSQT_BUCKETS)?,
            )
        };
        if l1 == 0 || !l1.is_multiple_of(64) || l1 > 4096 {
            return Err(format!("unsupported L1 = {l1}"));
        }
        let mut stacks = Vec::with_capacity(STACKS);
        for _ in 0..STACKS {
            let _h = r.u32()?;
            let b0: [i32; FC0_OUT] = r.i32s(FC0_OUT)?.try_into().unwrap();
            let raw0 = r.i8s(FC0_OUT * l1)?;
            let mut w0 = vec![0i8; FC0_OUT * l1];
            for o in 0..FC0_OUT {
                for j in 0..l1 {
                    w0[(j / 4) * 64 + o * 4 + j % 4] = raw0[o * l1 + j];
                }
            }
            let b1: [i32; FC1_OUT] = r.i32s(FC1_OUT)?.try_into().unwrap();
            let raw1 = r.i8s(FC1_OUT * FC1_IN)?;
            let mut w1 = [[0i8; FC1_IN]; FC1_OUT];
            for o in 0..FC1_OUT {
                w1[o].copy_from_slice(&raw1[o * FC1_IN..(o + 1) * FC1_IN]);
            }
            let mut w1s = [0i8; FC1_IN * FC1_OUT];
            for o in 0..FC1_OUT {
                for j in 0..FC1_IN {
                    w1s[(j / 4) * 128 + o * 4 + j % 4] = w1[o][j];
                }
            }
            let b2 = r.i32s(1)?[0];
            let w2: [i8; FC1_OUT] = r.i8s(FC1_OUT)?.try_into().unwrap();
            stacks.push(Stack {
                b0,
                w0,
                b1,
                w1,
                w1s,
                b2,
                w2,
            });
        }
        if r.o != bytes.len() {
            return Err("trailing bytes in network".into());
        }
        Ok(Network {
            l1,
            description,
            ft_bias,
            ft_w,
            psqt_w,
            stacks,
        })
    }

    #[inline(always)]
    fn row(&self, f: usize) -> &[i16] {
        &self.ft_w[f * self.l1..(f + 1) * self.l1]
    }

    #[inline(always)]
    fn prow(&self, f: usize) -> &[i32] {
        &self.psqt_w[f * PSQT_BUCKETS..(f + 1) * PSQT_BUCKETS]
    }

    /// Full evaluation from scratch (side-to-move relative, internal units).
    pub fn evaluate(&self, board: &Board) -> i32 {
        let mut acc = Acc::new(self.l1);
        for p in [WHITE, BLACK] {
            acc.refresh_from_scratch(self, board, p);
        }
        self.output(&acc, board)
    }

    /// Raw `(psqt + positional) / 16` as the reference engine reports it.
    fn output(&self, acc: &Acc, board: &Board) -> i32 {
        let pieces = board.occupied().count_ones() as usize;
        let bucket = (pieces - 1) / 4;
        let us = board.stm;
        let them = us ^ 1;
        let psqt = (acc.psqt[us][bucket] - acc.psqt[them][bucket]) / 2;
        let positional = self.propagate(acc, us, bucket);
        (psqt + positional) / OUTPUT_SCALE
    }

    fn propagate(&self, acc: &Acc, us: usize, bucket: usize) -> i32 {
        let l1 = self.l1;
        let half = l1 / 2;
        // Scratch buffers are fully written before being read (transform writes
        // all `l1` bytes; the scan writes `nz[..n]`), so skip zero-filling them.
        let mut input_buf = std::mem::MaybeUninit::<Aligned<[u8; 4096]>>::uninit();
        let mut nz_buf = std::mem::MaybeUninit::<[u16; 1024 + 16]>::uninit();
        // SAFETY: see above; `u8`/`u16` have no invalid bit patterns.
        let input_all = unsafe { &mut (*input_buf.as_mut_ptr()).0 };
        let nz = unsafe { &mut *nz_buf.as_mut_ptr() };
        let mut n = 0;
        for (k, p) in [us, us ^ 1].into_iter().enumerate() {
            let a = acc.side(p, l1);
            let (lo, hi) = a.split_at(half);
            let out = &mut input_all[k * half..(k + 1) * half];
            transform_half(lo, hi, out);
        }
        let input = &input_all[..l1];
        find_nonzero_chunks(input, nz, &mut n);
        let st = &self.stacks[bucket];
        let fc0 = fc0_sparse(input, &nz[..n], &st.w0, &st.b0);
        let mut x1 = [0u8; FC1_IN];
        for i in 0..FC0_OUT - 1 {
            let v = i64::from(fc0[i]);
            x1[i] = (((v * v) >> (2 * WEIGHT_SCALE_BITS)) / 128).min(127) as u8;
            x1[FC0_OUT - 1 + i] = (fc0[i] >> WEIGHT_SCALE_BITS).clamp(0, 127) as u8;
        }
        let fc1 = fc1_dense(&x1, st);
        let mut x2 = [0u8; FC1_OUT];
        for o in 0..FC1_OUT {
            x2[o] = (fc1[o] >> WEIGHT_SCALE_BITS).clamp(0, 127) as u8;
        }
        let out = st.b2 + dot_u8_i8(&x2, &st.w2);
        let fwd = fc0[FC0_OUT - 1] * (600 * OUTPUT_SCALE) / (127 * (1 << WEIGHT_SCALE_BITS));
        out + fwd
    }
}

#[repr(align(64))]
struct Aligned<T>(T);

/// `out[j] = clamp(lo[j], 0, 127) * clamp(hi[j], 0, 127) / 128`.
#[inline(always)]
fn transform_half(lo: &[i16], hi: &[i16], out: &mut [u8]) {
    #[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
    if lo.len().is_multiple_of(32) {
        // SAFETY: AVX2 enabled at compile time; all slices have length `lo.len()`.
        unsafe {
            use std::arch::x86_64::*;
            let zero = _mm256_setzero_si256();
            let max = _mm256_set1_epi16(127);
            for j in (0..lo.len()).step_by(32) {
                let ld = |s: &[i16], o: usize| _mm256_loadu_si256(s.as_ptr().add(o).cast());
                let a0 = _mm256_min_epi16(_mm256_max_epi16(ld(lo, j), zero), max);
                let a1 = _mm256_min_epi16(_mm256_max_epi16(ld(lo, j + 16), zero), max);
                let b0 = _mm256_min_epi16(_mm256_max_epi16(ld(hi, j), zero), max);
                let b1 = _mm256_min_epi16(_mm256_max_epi16(ld(hi, j + 16), zero), max);
                let p0 = _mm256_srli_epi16(_mm256_mullo_epi16(a0, b0), 7);
                let p1 = _mm256_srli_epi16(_mm256_mullo_epi16(a1, b1), 7);
                // packus interleaves 128-bit lanes; permute restores order.
                let packed = _mm256_permute4x64_epi64(_mm256_packus_epi16(p0, p1), 0b11_01_10_00);
                _mm256_storeu_si256(out.as_mut_ptr().add(j).cast(), packed);
            }
            return;
        }
    }
    for ((o, &x), &y) in out.iter_mut().zip(lo).zip(hi) {
        let x = i32::from(x).clamp(0, 127);
        let y = i32::from(y).clamp(0, 127);
        *o = ((x * y) >> 7) as u8;
    }
}

#[inline(always)]
fn fc1_dense(x: &[u8; FC1_IN], st: &Stack) -> [i32; FC1_OUT] {
    #[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
    {
        // SAFETY: AVX2 enabled; `w1s` holds FC1_IN/4 blocks of 128 bytes.
        unsafe {
            use std::arch::x86_64::*;
            let ones = _mm256_set1_epi16(1);
            let mut acc: [__m256i; 4] =
                std::array::from_fn(|g| _mm256_loadu_si256(st.b1.as_ptr().add(8 * g).cast()));
            let xp = x.as_ptr().cast::<i32>();
            for c in 0..FC1_IN / 4 {
                let xv = _mm256_set1_epi32(xp.add(c).read_unaligned());
                for (g, a) in acc.iter_mut().enumerate() {
                    let w = _mm256_loadu_si256(st.w1s.as_ptr().add(c * 128 + g * 32).cast());
                    *a = _mm256_add_epi32(*a, _mm256_madd_epi16(_mm256_maddubs_epi16(xv, w), ones));
                }
            }
            let mut out = [0i32; FC1_OUT];
            for (g, a) in acc.iter().enumerate() {
                _mm256_storeu_si256(out.as_mut_ptr().add(8 * g).cast(), *a);
            }
            out
        }
    }
    #[cfg(not(all(target_arch = "x86_64", target_feature = "avx2")))]
    {
        std::array::from_fn(|o| st.b1[o] + dot_u8_i8(x, &st.w1[o]))
    }
}

/// For each 8-bit mask, the positions of its set bits (padded).
const NZ_TABLE: [[u16; 8]; 256] = {
    let mut t = [[0u16; 8]; 256];
    let mut m = 0;
    while m < 256 {
        let mut k = 0;
        let mut b = 0;
        while b < 8 {
            if m & (1 << b) != 0 {
                t[m][k] = b as u16;
                k += 1;
            }
            b += 1;
        }
        m += 1;
    }
    t
};

/// Indices of non-zero 4-byte chunks of `input`.
#[inline(always)]
fn find_nonzero_chunks(input: &[u8], nz: &mut [u16], n: &mut usize) {
    #[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
    if input.len().is_multiple_of(32) {
        // SAFETY: AVX2 enabled; loads cover `input` exactly; nz has len >= input.len()/4 + 8.
        assert!(nz.len() >= input.len() / 4 + 8);
        unsafe {
            use std::arch::x86_64::*;
            let zero = _mm256_setzero_si256();
            for base in (0..input.len()).step_by(32) {
                let v = _mm256_loadu_si256(input.as_ptr().add(base).cast());
                let eq = _mm256_cmpeq_epi32(v, zero);
                let mask = !(_mm256_movemask_ps(_mm256_castsi256_ps(eq)) as u32) & 0xFF;
                let offs = _mm_loadu_si128(NZ_TABLE[mask as usize].as_ptr().cast());
                let idx = _mm_add_epi16(offs, _mm_set1_epi16((base / 4) as i16));
                // nz has 16 slack entries, so the 8-lane store never overruns.
                _mm_storeu_si128(nz.as_mut_ptr().add(*n).cast(), idx);
                *n += mask.count_ones() as usize;
            }
            return;
        }
    }
    for (c, w) in input.chunks_exact(4).enumerate() {
        nz[*n] = c as u16;
        *n += usize::from(u32::from_ne_bytes([w[0], w[1], w[2], w[3]]) != 0);
    }
}

/// `b0 + W0 * input` over the listed non-zero chunks.
#[inline(always)]
fn fc0_sparse(input: &[u8], nz: &[u16], w0: &[i8], b0: &[i32; FC0_OUT]) -> [i32; FC0_OUT] {
    #[cfg(all(
        target_arch = "x86_64",
        target_feature = "avx512vnni",
        target_feature = "avx512bw"
    ))]
    {
        // SAFETY: features are enabled at compile time; every chunk index is < L1/4
        // so all loads stay within `input` and `w0`.
        unsafe {
            use std::arch::x86_64::*;
            // Four independent accumulators hide the dpbusd latency chain.
            let mut acc = [
                _mm512_loadu_si512(b0.as_ptr().cast()),
                _mm512_setzero_si512(),
                _mm512_setzero_si512(),
                _mm512_setzero_si512(),
            ];
            let inp = input.as_ptr().cast::<i32>();
            let wp = w0.as_ptr();
            let step = |a: __m512i, c: u16| {
                let c = c as usize;
                let x = _mm512_set1_epi32(inp.add(c).read_unaligned());
                let w = _mm512_loadu_si512(wp.add(c * 64).cast());
                _mm512_dpbusd_epi32(a, x, w)
            };
            let mut it = nz.chunks_exact(4);
            for q in &mut it {
                acc[0] = step(acc[0], q[0]);
                acc[1] = step(acc[1], q[1]);
                acc[2] = step(acc[2], q[2]);
                acc[3] = step(acc[3], q[3]);
            }
            for &c in it.remainder() {
                acc[0] = step(acc[0], c);
            }
            let acc = _mm512_add_epi32(
                _mm512_add_epi32(acc[0], acc[1]),
                _mm512_add_epi32(acc[2], acc[3]),
            );
            let mut out = [0i32; FC0_OUT];
            _mm512_storeu_si512(out.as_mut_ptr().cast(), acc);
            out
        }
    }
    #[cfg(all(
        target_arch = "x86_64",
        target_feature = "avx2",
        not(all(target_feature = "avx512vnni", target_feature = "avx512bw"))
    ))]
    {
        // SAFETY: as above; products of u8 <= 127 and i8 fit maddubs without saturation.
        unsafe {
            use std::arch::x86_64::*;
            let ones = _mm256_set1_epi16(1);
            let mut a0 = _mm256_loadu_si256(b0.as_ptr().cast());
            let mut a1 = _mm256_loadu_si256(b0.as_ptr().add(8).cast());
            let (mut c0, mut c1) = (_mm256_setzero_si256(), _mm256_setzero_si256());
            let inp = input.as_ptr().cast::<i32>();
            let prod = |c: u16| {
                let c = c as usize;
                let x = _mm256_set1_epi32(inp.add(c).read_unaligned());
                let w = w0.as_ptr().add(c * 64);
                let w_lo = _mm256_loadu_si256(w.cast());
                let w_hi = _mm256_loadu_si256(w.add(32).cast());
                (
                    _mm256_madd_epi16(_mm256_maddubs_epi16(x, w_lo), ones),
                    _mm256_madd_epi16(_mm256_maddubs_epi16(x, w_hi), ones),
                )
            };
            let mut it = nz.chunks_exact(2);
            for q in &mut it {
                let (l, h) = prod(q[0]);
                a0 = _mm256_add_epi32(a0, l);
                a1 = _mm256_add_epi32(a1, h);
                let (l, h) = prod(q[1]);
                c0 = _mm256_add_epi32(c0, l);
                c1 = _mm256_add_epi32(c1, h);
            }
            for &c in it.remainder() {
                let (l, h) = prod(c);
                a0 = _mm256_add_epi32(a0, l);
                a1 = _mm256_add_epi32(a1, h);
            }
            a0 = _mm256_add_epi32(a0, c0);
            a1 = _mm256_add_epi32(a1, c1);
            let mut out = [0i32; FC0_OUT];
            _mm256_storeu_si256(out.as_mut_ptr().cast(), a0);
            _mm256_storeu_si256(out.as_mut_ptr().add(8).cast(), a1);
            out
        }
    }
    #[cfg(not(all(target_arch = "x86_64", target_feature = "avx2")))]
    {
        let mut out = *b0;
        for &c in nz {
            let c = c as usize;
            for o in 0..FC0_OUT {
                for k in 0..4 {
                    out[o] += i32::from(input[4 * c + k]) * i32::from(w0[c * 64 + o * 4 + k]);
                }
            }
        }
        out
    }
}

#[inline(always)]
fn dot_u8_i8(a: &[u8], w: &[i8]) -> i32 {
    a.iter()
        .zip(w)
        .map(|(&x, &y)| i32::from(x) * i32::from(y))
        .sum()
}

/// One position's accumulators: `[2][L1]` int16 plus `[2][8]` PSQT sums.
#[derive(Clone)]
pub struct Acc {
    v: Vec<i16>,
    psqt: [[i32; PSQT_BUCKETS]; 2],
    /// Whether `v`/`psqt` are up to date for each perspective (lazy updates).
    computed: [bool; 2],
    dirty: Dirty,
    board: Option<Board>,
}

/// Feature changes from the parent ply: up to two (code, square) adds/subs.
/// `king_side` is the perspective whose king moved (needs a refresh), or 2.
#[derive(Clone, Copy, Default)]
struct Dirty {
    adds: [(usize, Square); 2],
    subs: [(usize, Square); 2],
    na: usize,
    ns: usize,
    king_side: usize,
}

impl Acc {
    fn new(l1: usize) -> Self {
        Acc {
            v: vec![0; 2 * l1],
            psqt: [[0; PSQT_BUCKETS]; 2],
            computed: [false; 2],
            dirty: Dirty::default(),
            board: None,
        }
    }
    #[inline(always)]
    fn side(&self, p: usize, l1: usize) -> &[i16] {
        &self.v[p * l1..(p + 1) * l1]
    }
    #[inline(always)]
    fn side_mut(&mut self, p: usize, l1: usize) -> &mut [i16] {
        &mut self.v[p * l1..(p + 1) * l1]
    }

    fn refresh_from_scratch(&mut self, net: &Network, board: &Board, p: usize) {
        let l1 = net.l1;
        let ksq = board.king_sq(p);
        self.side_mut(p, l1).copy_from_slice(&net.ft_bias);
        self.psqt[p] = [0; PSQT_BUCKETS];
        let mut occ = board.occupied();
        while occ != 0 {
            let sq = pop_lsb(&mut occ);
            let f = feature(p, ksq, board.mailbox[sq] as usize, sq);
            add_row(self.side_mut(p, l1), net.row(f));
            for (a, w) in self.psqt[p].iter_mut().zip(net.prow(f)) {
                *a += w;
            }
        }
    }
}

#[inline(always)]
fn add_row(dst: &mut [i16], row: &[i16]) {
    for (d, w) in dst.iter_mut().zip(row) {
        *d = d.wrapping_add(*w);
    }
}

#[inline(always)]
fn sub_row(dst: &mut [i16], row: &[i16]) {
    for (d, w) in dst.iter_mut().zip(row) {
        *d = d.wrapping_sub(*w);
    }
}

/// Fused copy + update: `dst = src + sum(adds) - sum(subs)`.
#[inline(always)]
fn update(dst: &mut [i16], src: &[i16], adds: &[&[i16]], subs: &[&[i16]]) {
    let n = dst.len();
    let src = &src[..n];
    match (adds.len(), subs.len()) {
        (1, 1) => {
            let (a, s) = (&adds[0][..n], &subs[0][..n]);
            for (((d, &x), &a), &s) in dst.iter_mut().zip(src).zip(a).zip(s) {
                *d = x.wrapping_add(a).wrapping_sub(s);
            }
        }
        (1, 2) => {
            let (a, s1, s2) = (&adds[0][..n], &subs[0][..n], &subs[1][..n]);
            for ((((d, &x), &a), &s1), &s2) in dst.iter_mut().zip(src).zip(a).zip(s1).zip(s2) {
                *d = x.wrapping_add(a).wrapping_sub(s1).wrapping_sub(s2);
            }
        }
        (2, 2) => {
            let (a1, a2) = (&adds[0][..n], &adds[1][..n]);
            let (s1, s2) = (&subs[0][..n], &subs[1][..n]);
            for (((((d, &x), &a1), &a2), &s1), &s2) in
                dst.iter_mut().zip(src).zip(a1).zip(a2).zip(s1).zip(s2)
            {
                *d = x
                    .wrapping_add(a1)
                    .wrapping_add(a2)
                    .wrapping_sub(s1)
                    .wrapping_sub(s2);
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

/// "Finny table" entry: the accumulator of the last board refreshed for this
/// (perspective, king bucket, mirror) together with that board's pieces, so a
/// king move only applies the piece difference instead of a full refresh.
#[derive(Clone)]
struct CacheEntry {
    v: Vec<i16>,
    psqt: [i32; PSQT_BUCKETS],
    pieces: [Bitboard; 12],
}

/// Per-thread accumulator stack plus refresh cache.
pub struct Accumulators {
    stack: Vec<Acc>,
    cache: Vec<CacheEntry>,
    l1: usize,
}

impl Default for Accumulators {
    fn default() -> Self {
        Self::new()
    }
}

impl Accumulators {
    pub fn new() -> Self {
        Accumulators {
            stack: Vec::new(),
            cache: Vec::new(),
            l1: 0,
        }
    }

    fn ensure(&mut self, net: &Network) {
        if self.l1 != net.l1 || self.stack.is_empty() {
            self.l1 = net.l1;
            self.stack = (0..crate::search::MAX_PLY + 8)
                .map(|_| Acc::new(net.l1))
                .collect();
            self.reset_cache(net);
        }
    }

    fn reset_cache(&mut self, net: &Network) {
        let e = CacheEntry {
            v: net.ft_bias.clone(),
            psqt: [0; PSQT_BUCKETS],
            pieces: [0; 12],
        };
        self.cache = vec![e; 2 * 64];
    }

    /// Refresh perspective `p` of `stack[ply]` from `board` via the cache.
    fn refresh_side(&mut self, net: &Network, board: &Board, ply: usize, p: usize) {
        let l1 = net.l1;
        let ksq = board.king_sq(p);
        let idx = p * 64 + king_bucket(p, ksq) * 2 + usize::from(file_of(ksq) < 4);
        let entry = &mut self.cache[idx];
        for code in 0..12 {
            let now = board.colors[code / 6] & board.pieces[code % 6];
            let before = entry.pieces[code];
            let mut added = now & !before;
            let mut removed = before & !now;
            while added != 0 {
                let sq = pop_lsb(&mut added);
                let f = feature(p, ksq, code, sq);
                add_row(&mut entry.v, net.row(f));
                for (a, w) in entry.psqt.iter_mut().zip(net.prow(f)) {
                    *a += w;
                }
            }
            while removed != 0 {
                let sq = pop_lsb(&mut removed);
                let f = feature(p, ksq, code, sq);
                sub_row(&mut entry.v, net.row(f));
                for (a, w) in entry.psqt.iter_mut().zip(net.prow(f)) {
                    *a -= w;
                }
            }
            entry.pieces[code] = now;
        }
        let acc = &mut self.stack[ply];
        acc.side_mut(p, l1).copy_from_slice(&entry.v);
        acc.psqt[p] = entry.psqt;
    }

    pub fn refresh(&mut self, net: &Network, board: &Board, ply: usize) {
        self.ensure(net);
        for p in [WHITE, BLACK] {
            self.refresh_side(net, board, ply, p);
        }
        self.stack[ply].computed = [true; 2];
        self.stack[ply].board = Some(*board);
    }

    /// Record the move; the accumulator itself is updated lazily on evaluation.
    pub fn push(&mut self, _net: &Network, parent: &Board, child: &Board, mv: Move, ply: usize) {
        let us = parent.stm;
        let from = mv.from();
        let to = mv.to();
        let code = parent.mailbox[from] as usize;
        let placed = if mv.is_promotion() {
            us * 6 + mv.promotion()
        } else {
            code
        };
        let mut d = Dirty {
            adds: [(placed, to), (0, 0)],
            subs: [(code, from), (0, 0)],
            na: 1,
            ns: 1,
            king_side: if code % 6 == KING { us } else { 2 },
        };
        if mv.is_castle() {
            let (rf, rt) = if mv.flag() == KING_CASTLE {
                (to + 1, to - 1)
            } else {
                (to - 2, to + 1)
            };
            d.adds[1] = (us * 6 + ROOK, rt);
            d.subs[1] = (us * 6 + ROOK, rf);
            d.na = 2;
            d.ns = 2;
        } else if mv.is_capture() {
            d.subs[1] = if mv.flag() == EN_PASSANT {
                ((us ^ 1) * 6 + PAWN, to ^ 8)
            } else {
                (parent.mailbox[to] as usize, to)
            };
            d.ns = 2;
        }
        let e = &mut self.stack[ply];
        e.dirty = d;
        e.computed = [false; 2];
        e.board = Some(*child);
    }

    pub fn push_null(&mut self, ply: usize) {
        let parent_board = self.stack[ply - 1].board;
        let e = &mut self.stack[ply];
        e.dirty = Dirty {
            king_side: 2,
            ..Dirty::default()
        };
        e.computed = [false; 2];
        e.board = parent_board.map(|b| b.make_null());
    }

    /// Apply `stack[ply].dirty` to perspective `p` on top of `stack[ply - 1]`.
    fn apply(&mut self, net: &Network, ply: usize, p: usize) {
        let l1 = net.l1;
        let d = self.stack[ply].dirty;
        let ksq = self.stack[ply].board.as_ref().unwrap().king_sq(p);
        let fa: [usize; 2] = std::array::from_fn(|i| feature(p, ksq, d.adds[i].0, d.adds[i].1));
        let fs: [usize; 2] = std::array::from_fn(|i| feature(p, ksq, d.subs[i].0, d.subs[i].1));
        let ra: [&[i16]; 2] = [net.row(fa[0]), net.row(fa[1])];
        let rs: [&[i16]; 2] = [net.row(fs[0]), net.row(fs[1])];
        let (prev, rest) = self.stack.split_at_mut(ply);
        let src = &prev[ply - 1];
        let dst = &mut rest[0];
        update(
            dst.side_mut(p, l1),
            src.side(p, l1),
            &ra[..d.na],
            &rs[..d.ns],
        );
        let mut ps = src.psqt[p];
        for &f in &fa[..d.na] {
            for (a, w) in ps.iter_mut().zip(net.prow(f)) {
                *a += w;
            }
        }
        for &f in &fs[..d.ns] {
            for (a, w) in ps.iter_mut().zip(net.prow(f)) {
                *a -= w;
            }
        }
        dst.psqt[p] = ps;
        dst.computed[p] = true;
    }

    fn materialize(&mut self, net: &Network, ply: usize, p: usize) {
        if self.stack[ply].computed[p] {
            return;
        }
        let mut k = ply;
        while !self.stack[k].computed[p] && self.stack[k].dirty.king_side != p {
            k -= 1;
        }
        if !self.stack[k].computed[p] {
            let b = self.stack[k].board.unwrap();
            self.refresh_side(net, &b, k, p);
            self.stack[k].computed[p] = true;
        }
        for j in k + 1..=ply {
            self.apply(net, j, p);
        }
    }

    /// Side-to-move relative raw network value (reference-engine units).
    pub fn evaluate(&mut self, net: &Network, board: &Board, ply: usize) -> i32 {
        self.materialize(net, ply, WHITE);
        self.materialize(net, ply, BLACK);
        net.output(&self.stack[ply], board)
    }
}

#[doc(hidden)]
pub fn profile(net: &Network, board: &Board) {
    use std::hint::black_box;
    use std::time::Instant;
    let mut acc = Acc::new(net.l1);
    for p in [WHITE, BLACK] {
        acc.refresh_from_scratch(net, board, p);
    }
    let l1 = net.l1;
    let half = l1 / 2;
    let n_iter = 1_000_000;
    let mut input = Aligned([0u8; 4096]);
    let t = Instant::now();
    for _ in 0..n_iter {
        for (k, p) in [WHITE, BLACK].into_iter().enumerate() {
            let a = black_box(&acc).side(p, l1);
            let (lo, hi) = a.split_at(half);
            transform_half(lo, hi, &mut input.0[k * half..(k + 1) * half]);
        }
        black_box(&input);
    }
    let tt = t.elapsed().as_nanos() as f64 / n_iter as f64;
    let mut nz = [0u16; 1024 + 16];
    let mut n = 0;
    let t = Instant::now();
    for _ in 0..n_iter {
        n = 0;
        find_nonzero_chunks(black_box(&input.0[..l1]), &mut nz, &mut n);
        black_box(&nz);
    }
    let tn = t.elapsed().as_nanos() as f64 / n_iter as f64;
    let st = &net.stacks[0];
    let t = Instant::now();
    let mut s = 0;
    for _ in 0..n_iter {
        s += fc0_sparse(
            black_box(&input.0[..l1]),
            black_box(&nz[..n]),
            &st.w0,
            &st.b0,
        )[3];
    }
    let tf = t.elapsed().as_nanos() as f64 / n_iter as f64;
    let t = Instant::now();
    for _ in 0..n_iter {
        s += net.propagate(black_box(&acc), WHITE, 0);
    }
    let tp = t.elapsed().as_nanos() as f64 / n_iter as f64;
    eprintln!("transform {tt:.1} ns, nonzero {tn:.1} ns ({n} chunks), fc0 {tf:.1} ns, propagate {tp:.1} ns [{s}]");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::movegen::legal_moves;

    /// Serialized random network with the given L1 in the published layout.
    pub fn synthetic(l1: usize) -> Vec<u8> {
        let mut s = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move |m: i64| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s % (2 * m as u64 + 1)) as i64 - m
        };
        let mut b = Vec::new();
        let desc = b"synthetic";
        for v in [0x7AF3_2F20u32, 0, desc.len() as u32] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(desc);
        b.extend_from_slice(&0u32.to_le_bytes());
        for _ in 0..l1 {
            b.extend_from_slice(&(next(40) as i16).to_le_bytes());
        }
        for _ in 0..INPUTS * l1 {
            b.extend_from_slice(&(next(30) as i16).to_le_bytes());
        }
        for _ in 0..INPUTS * PSQT_BUCKETS {
            b.extend_from_slice(&(next(3000) as i32).to_le_bytes());
        }
        for _ in 0..STACKS {
            b.extend_from_slice(&0u32.to_le_bytes());
            for _ in 0..FC0_OUT {
                b.extend_from_slice(&(next(2000) as i32).to_le_bytes());
            }
            for _ in 0..FC0_OUT * l1 {
                b.push(next(60) as i8 as u8);
            }
            for _ in 0..FC1_OUT {
                b.extend_from_slice(&(next(2000) as i32).to_le_bytes());
            }
            for j in 0..FC1_OUT * FC1_IN {
                b.push(if j % FC1_IN < 30 {
                    next(60) as i8 as u8
                } else {
                    0
                });
            }
            b.extend_from_slice(&(next(2000) as i32).to_le_bytes());
            for _ in 0..FC1_OUT {
                b.push(next(60) as i8 as u8);
            }
        }
        b
    }

    #[test]
    fn incremental_updates_and_cache_match_refresh() {
        let net = Network::from_bytes(&synthetic(64)).unwrap();
        assert_eq!(net.l1, 64);
        let mut acc = Accumulators::new();
        let mut seed = 11u64;
        for fen in [
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
            "rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPP1PPP/RNBQKBNR w KQkq f6 0 3",
            "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        ] {
            let mut board = Board::from_fen(fen).unwrap();
            acc.refresh(&net, &board, 0);
            for ply in 1..60 {
                let moves = legal_moves(&board);
                if moves.is_empty() {
                    break;
                }
                seed = seed
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let special: Vec<_> = moves
                    .iter()
                    .copied()
                    .filter(|m| m.is_noisy() || m.is_castle())
                    .collect();
                let pool = if !special.is_empty() && (seed >> 20).is_multiple_of(3) {
                    &special
                } else {
                    &moves
                };
                let mv = pool[(seed >> 33) as usize % pool.len()];
                let child = board.make_move(mv);
                acc.push(&net, &board, &child, mv, ply);
                assert_eq!(
                    acc.evaluate(&net, &child, ply),
                    net.evaluate(&child),
                    "{fen} {}",
                    mv.to_uci()
                );
                board = child;
            }
        }
    }

    #[test]
    fn lazy_chains_with_skipped_evals_and_null_moves() {
        let net = Network::from_bytes(&synthetic(64)).unwrap();
        let mut acc = Accumulators::new();
        let mut board =
            Board::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1")
                .unwrap();
        acc.refresh(&net, &board, 0);
        let mut seed = 5u64;
        for ply in 1..100 {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let moves = legal_moves(&board);
            if moves.is_empty() {
                break;
            }
            let child = if (seed >> 40).is_multiple_of(5) && !board.in_check() {
                acc.push_null(ply);
                board.make_null()
            } else {
                let mv = moves[(seed >> 33) as usize % moves.len()];
                let c = board.make_move(mv);
                acc.push(&net, &board, &c, mv, ply);
                c
            };
            if (seed >> 50).is_multiple_of(4) {
                assert_eq!(
                    acc.evaluate(&net, &child, ply),
                    net.evaluate(&child),
                    "ply {ply}"
                );
            }
            board = child;
        }
    }

    #[test]
    fn rejects_malformed_files() {
        let mut bytes = synthetic(64);
        bytes.pop();
        assert!(Network::from_bytes(&bytes).is_err());
        assert!(Network::from_bytes(b"FZN1........").is_err());
    }

    #[test]
    fn feature_tables_match_reference_layout() {
        // White king e1 -> bucket 31, no mirror; a1 -> bucket 28, mirrored.
        assert_eq!(king_bucket(WHITE, 4), 31);
        assert_eq!(king_bucket(WHITE, 0), 28);
        assert_eq!(king_bucket(WHITE, 56), 0);
        assert_eq!(king_bucket(BLACK, 60), 31);
        assert_eq!(king_bucket(BLACK, 0), 0);
        assert_eq!(orient(WHITE, 4), 0);
        assert_eq!(orient(WHITE, 3), 7);
        assert_eq!(orient(BLACK, 60), 56);
        assert_eq!(orient(BLACK, 59), 63);
        // White pawn on e2 from white's view with king e1.
        assert_eq!(feature(WHITE, 4, PAWN, 12), 12 + 31 * 704);
        // Black king as seen by white.
        assert_eq!(feature(WHITE, 4, 6 + KING, 60), 60 + 640 + 31 * 704);
    }
}
