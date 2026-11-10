//! Farzin NNUE inference: (768 -> H)x2 -> 1 with SCReLU and material-based
//! output buckets. Network files use Farzin's own "FZN1" format, produced by
//! `farzin-train` from self-play data.
use crate::board::Board;
use crate::types::*;

pub const INPUTS: usize = 768;
pub const QA: i32 = 255;
pub const QB: i32 = 64;
pub const SCALE: i32 = 400;
pub const MAX_HIDDEN: usize = 1024;
const MAGIC: &[u8; 4] = b"FZN1";

pub struct Network {
    pub hidden: usize,
    pub buckets: usize,
    /// `[INPUTS][hidden]`
    pub ft_weights: Vec<i16>,
    pub ft_bias: Vec<i16>,
    /// `[buckets][2 * hidden]`
    pub out_weights: Vec<i16>,
    pub out_bias: Vec<i32>,
}

#[inline(always)]
pub fn feature(perspective: usize, code: usize, sq: Square) -> usize {
    let color = code / 6;
    let piece = code % 6;
    let rel_color = usize::from(color != perspective);
    let rel_sq = if perspective == WHITE { sq } else { sq ^ 56 };
    rel_color * 384 + piece * 64 + rel_sq
}

#[inline(always)]
pub fn output_bucket(board: &Board, buckets: usize) -> usize {
    let count = board.occupied().count_ones() as usize;
    ((count - 2) * buckets / 31).min(buckets - 1)
}

impl Network {
    pub fn from_bytes(bytes: &[u8]) -> Result<Network, String> {
        if bytes.len() < 12 || &bytes[..4] != MAGIC {
            return Err("not a Farzin FZN1 network".into());
        }
        let u32_at = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap()) as usize;
        let hidden = u32_at(4);
        let buckets = u32_at(8);
        if hidden == 0 || hidden > MAX_HIDDEN || hidden % 16 != 0 || buckets == 0 || buckets > 16 {
            return Err(format!("unsupported network shape {hidden}x{buckets}"));
        }
        let expected = 12 + 2 * (INPUTS * hidden + hidden + buckets * 2 * hidden) + 4 * buckets;
        if bytes.len() != expected {
            return Err(format!(
                "network size {} != expected {expected}",
                bytes.len()
            ));
        }
        let mut offset = 12;
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
        let ft_weights = read_i16(INPUTS * hidden);
        let ft_bias = read_i16(hidden);
        let out_weights = read_i16(buckets * 2 * hidden);
        let out_bias = bytes[offset..offset + 4 * buckets]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| i32::from_le_bytes(*c))
            .collect();
        Ok(Network {
            hidden,
            buckets,
            ft_weights,
            ft_bias,
            out_weights,
            out_bias,
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&(self.hidden as u32).to_le_bytes());
        out.extend_from_slice(&(self.buckets as u32).to_le_bytes());
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

    /// Full (non-incremental) evaluation, for tests and tools.
    pub fn evaluate(&self, board: &Board) -> i32 {
        let mut acc = Accumulator::new(self.hidden);
        acc.refresh(self, board);
        acc.output(self, board)
    }
}

#[derive(Clone)]
pub struct Accumulator {
    pub v: [Vec<i16>; 2],
}

impl Accumulator {
    fn new(hidden: usize) -> Self {
        Accumulator {
            v: [vec![0; hidden], vec![0; hidden]],
        }
    }

    fn refresh(&mut self, net: &Network, board: &Board) {
        for p in [WHITE, BLACK] {
            self.v[p].copy_from_slice(&net.ft_bias);
        }
        let mut occ = board.occupied();
        while occ != 0 {
            let sq = pop_lsb(&mut occ);
            let code = board.mailbox[sq] as usize;
            for p in [WHITE, BLACK] {
                let row = net.row(feature(p, code, sq));
                for (a, w) in self.v[p].iter_mut().zip(row) {
                    *a = a.wrapping_add(*w);
                }
            }
        }
    }

    #[inline]
    fn output(&self, net: &Network, board: &Board) -> i32 {
        let h = net.hidden;
        let bucket = output_bucket(board, net.buckets);
        let weights = &net.out_weights[bucket * 2 * h..(bucket + 1) * 2 * h];
        let us = board.stm;
        let sum =
            screlu_dot(&self.v[us], &weights[..h]) + screlu_dot(&self.v[us ^ 1], &weights[h..]);
        let raw = sum / QA + net.out_bias[bucket];
        raw * SCALE / (QA * QB)
    }
}

#[inline]
fn screlu_dot(acc: &[i16], weights: &[i16]) -> i32 {
    let mut sum = 0i32;
    for (a, w) in acc.iter().zip(weights) {
        let v = i32::from(*a).clamp(0, QA);
        sum += v * v * i32::from(*w);
    }
    sum
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
                for i in 0..dst.len() {
                    dst[i] = dst[i].wrapping_add(a[i]);
                }
            }
            for s in subs {
                for i in 0..dst.len() {
                    dst[i] = dst[i].wrapping_sub(s[i]);
                }
            }
        }
    }
}

/// Per-ply accumulator stack updated incrementally along the search path.
pub struct Accumulators {
    stack: Vec<Accumulator>,
    hidden: usize,
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
            hidden: 0,
        }
    }

    fn ensure(&mut self, net: &Network) {
        if self.hidden != net.hidden || self.stack.is_empty() {
            self.hidden = net.hidden;
            self.stack = (0..crate::search::MAX_PLY + 8)
                .map(|_| Accumulator::new(net.hidden))
                .collect();
        }
    }

    pub fn refresh(&mut self, net: &Network, board: &Board, ply: usize) {
        self.ensure(net);
        self.stack[ply].refresh(net, board);
    }

    /// Accumulator for `child` (= `parent` after `mv`) at `ply`.
    pub fn push(&mut self, net: &Network, parent: &Board, _child: &Board, mv: Move, ply: usize) {
        let us = parent.stm;
        let from = mv.from();
        let to = mv.to();
        let code = parent.mailbox[from] as usize;
        let placed = if mv.is_promotion() {
            us * 6 + mv.promotion()
        } else {
            code
        };
        let (prev, rest) = self.stack.split_at_mut(ply);
        let src = &prev[ply - 1];
        let dst = &mut rest[0];
        for p in [WHITE, BLACK] {
            let add1 = net.row(feature(p, placed, to));
            let sub1 = net.row(feature(p, code, from));
            if mv.is_castle() {
                let (rf, rt) = if mv.flag() == KING_CASTLE {
                    (to + 1, to - 1)
                } else {
                    (to - 2, to + 1)
                };
                let rook = us * 6 + ROOK;
                let add2 = net.row(feature(p, rook, rt));
                let sub2 = net.row(feature(p, rook, rf));
                apply(&mut dst.v[p], &src.v[p], &[add1, add2], &[sub1, sub2]);
            } else if mv.is_capture() {
                let (cap_sq, cap_code) = if mv.flag() == EN_PASSANT {
                    (to ^ 8, (us ^ 1) * 6 + PAWN)
                } else {
                    (to, parent.mailbox[to] as usize)
                };
                let sub2 = net.row(feature(p, cap_code, cap_sq));
                apply(&mut dst.v[p], &src.v[p], &[add1], &[sub1, sub2]);
            } else {
                apply(&mut dst.v[p], &src.v[p], &[add1], &[sub1]);
            }
        }
    }

    pub fn push_null(&mut self, _net: &Network, ply: usize) {
        let (prev, rest) = self.stack.split_at_mut(ply);
        let src = &prev[ply - 1];
        for p in [WHITE, BLACK] {
            rest[0].v[p].copy_from_slice(&src.v[p]);
        }
    }

    pub fn evaluate(&self, net: &Network, board: &Board, ply: usize) -> i32 {
        self.stack[ply].output(net, board)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::movegen::legal_moves;

    fn random_net(hidden: usize, buckets: usize) -> Network {
        let mut s = 0x1234_5678u64;
        let mut next = move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            ((s >> 40) % 200) as i16 - 100
        };
        Network {
            hidden,
            buckets,
            ft_weights: (0..INPUTS * hidden).map(|_| next()).collect(),
            ft_bias: (0..hidden).map(|_| next()).collect(),
            out_weights: (0..buckets * 2 * hidden).map(|_| next() / 2).collect(),
            out_bias: (0..buckets).map(|_| i32::from(next()) * 100).collect(),
        }
    }

    #[test]
    fn incremental_updates_match_refresh_and_serialization_round_trips() {
        let net = random_net(32, 8);
        let reloaded = Network::from_bytes(&net.to_bytes()).unwrap();
        assert_eq!(reloaded.ft_weights, net.ft_weights);
        assert_eq!(reloaded.out_bias, net.out_bias);
        let mut acc = Accumulators::new();
        for fen in [
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
            "rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPP1PPP/RNBQKBNR w KQkq f6 0 3",
        ] {
            let mut board = Board::from_fen(fen).unwrap();
            acc.refresh(&net, &board, 0);
            let mut seed = 7usize;
            for ply in 1..40 {
                let moves = legal_moves(&board);
                if moves.is_empty() {
                    break;
                }
                // Prefer special moves to exercise castling, ep and promotion paths.
                let special: Vec<_> = moves
                    .iter()
                    .copied()
                    .filter(|m| m.is_noisy() || m.is_castle())
                    .collect();
                seed = seed
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let pool = if !special.is_empty() && seed.is_multiple_of(3) {
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
                    "{} {}",
                    fen,
                    mv.to_uci()
                );
                board = child;
            }
        }
    }
}
