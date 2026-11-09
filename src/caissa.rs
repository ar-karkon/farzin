//! Experimental Caissa v15 scalar inference.
//!
//! File layout and mathematics refer to Caissa commit 395c2e6 by Michał
//! Witanowski (MIT); see THIRD_PARTY_NOTICES.md. Weights are not distributed.
use chess::{Board, Color, Piece};
use std::path::Path;
use std::sync::Arc;

const FILE_SIZE: usize = 75_703_872;
const MAGIC: u32 = 0x43534e4e;
const VERSION: u32 = 15;
const WIDTH: usize = 1536;
const HEADER: usize = 64;
const INPUTS: usize = 32 * 12 * 64;
const BIAS_OFFSET: usize = HEADER + INPUTS * WIDTH * 2;
const SUBNET_OFFSET: usize = BIAS_OFFSET + WIDTH * 2;
const SUBNET_SIZE: usize = 25_408;

pub struct Network {
    weights: Vec<i16>,
    bias: [i16; WIDTH],
    bytes: Vec<u8>,
}

/// Per-search cache. Diffs from the last evaluated board, so skipped nodes and
/// search interruption do not require separate make/unmake synchronization.
pub struct CachedEvaluator {
    network: Arc<Network>,
    board: Board,
    white: [i16; WIDTH],
    black: [i16; WIDTH],
}

impl CachedEvaluator {
    pub fn new(network: Arc<Network>, board: &Board) -> Self {
        let white = network.accumulator(board, Color::White);
        let black = network.accumulator(board, Color::Black);
        Self {
            network,
            board: *board,
            white,
            black,
        }
    }

    pub fn evaluate(&mut self, board: &Board) -> i32 {
        if self.board != *board {
            self.network
                .transition(&mut self.white, &self.board, board, Color::White);
            self.network
                .transition(&mut self.black, &self.board, board, Color::Black);
            self.board = *board;
        }
        self.network
            .evaluate_accumulators(board, &self.white, &self.black)
    }
}

/// Lazy per-ply state. Storage is allocated once, never while making moves.
/// Board deltas include castling, en passant and promotions without move decoding.
struct Frame {
    board: Board,
    white: [i16; WIDTH],
    black: [i16; WIDTH],
    valid: bool,
}

pub struct SearchEvaluator {
    network: Arc<Network>,
    frames: Vec<Frame>,
    ply: usize,
}

impl SearchEvaluator {
    pub fn new(network: Arc<Network>, board: &Board) -> Self {
        let mut frames: Vec<_> = (0..128)
            .map(|_| Frame {
                board: *board,
                white: [0; WIDTH],
                black: [0; WIDTH],
                valid: false,
            })
            .collect();
        frames[0].white = network.accumulator(board, Color::White);
        frames[0].black = network.accumulator(board, Color::Black);
        frames[0].valid = true;
        Self {
            network,
            frames,
            ply: 0,
        }
    }

    pub fn push(&mut self, board: &Board) {
        self.ply += 1;
        let frame = self.frames.get_mut(self.ply).expect("NNUE stack overflow");
        frame.board = *board;
        frame.valid = false;
    }

    pub fn pop(&mut self) {
        self.ply = self.ply.checked_sub(1).expect("NNUE stack underflow");
    }

    fn ensure(&mut self) {
        let mut start = self.ply;
        while !self.frames[start].valid {
            start -= 1;
        }
        for index in start + 1..=self.ply {
            let (parents, children) = self.frames.split_at_mut(index);
            let parent = &parents[index - 1];
            let child = &mut children[0];
            child.white = parent.white;
            child.black = parent.black;
            self.network
                .transition(&mut child.white, &parent.board, &child.board, Color::White);
            self.network
                .transition(&mut child.black, &parent.board, &child.board, Color::Black);
            child.valid = true;
        }
    }

    pub fn evaluate(&mut self, board: &Board) -> i32 {
        assert_eq!(
            self.frames[self.ply].board, *board,
            "NNUE stack desynchronized"
        );
        self.ensure();
        let frame = &self.frames[self.ply];
        self.network
            .evaluate_accumulators(board, &frame.white, &frame.black)
    }
}

/// One perspective's incrementally maintained feature transformer state.
pub struct PerspectiveAccumulator<'a> {
    network: &'a Network,
    perspective: Color,
    features: Vec<usize>,
    values: [i16; WIDTH],
}

impl<'a> PerspectiveAccumulator<'a> {
    pub fn new(network: &'a Network, board: &Board, perspective: Color) -> Self {
        Self {
            network,
            perspective,
            features: feature_indices(board, perspective),
            values: network.accumulator(board, perspective),
        }
    }

    pub fn values(&self) -> &[i16; WIDTH] {
        &self.values
    }

    /// Reconcile feature sets; king bucket and mirror changes are handled by the same diff.
    pub fn update(&mut self, board: &Board) {
        let next = feature_indices(board, self.perspective);
        for index in 0..self.features.len() {
            let feature = self.features[index];
            if next.binary_search(&feature).is_err() {
                self.apply(feature, false);
            }
        }
        for &feature in &next {
            if self.features.binary_search(&feature).is_err() {
                self.apply(feature, true);
            }
        }
        self.features = next;
    }

    fn apply(&mut self, feature: usize, add: bool) {
        let row = &self.network.weights[feature * WIDTH..(feature + 1) * WIDTH];
        for (value, &weight) in self.values.iter_mut().zip(row) {
            *value = if add {
                value.wrapping_add(weight)
            } else {
                value.wrapping_sub(weight)
            };
        }
    }
}

impl Network {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
        if size != FILE_SIZE as u64 {
            return Err(format!("invalid network size: {size}"));
        }
        Self::from_bytes(std::fs::read(path).map_err(|e| e.to_string())?)
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, String> {
        if bytes.len() != FILE_SIZE {
            return Err("invalid network size".into());
        }
        if u32::from_le_bytes(bytes[0..4].try_into().unwrap()) != MAGIC {
            return Err("invalid network magic".into());
        }
        if u32::from_le_bytes(bytes[4..8].try_into().unwrap()) != VERSION {
            return Err("unsupported network version".into());
        }
        let weights = bytes[HEADER..BIAS_OFFSET]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        let bias = std::array::from_fn(|i| {
            i16::from_le_bytes([bytes[BIAS_OFFSET + i * 2], bytes[BIAS_OFFSET + i * 2 + 1]])
        });
        let subnets = bytes[SUBNET_OFFSET..].to_vec();
        Ok(Self {
            weights,
            bias,
            bytes: subnets,
        })
    }

    fn i16_at(&self, offset: usize) -> i16 {
        i16::from_le_bytes([self.bytes[offset], self.bytes[offset + 1]])
    }
    fn i32_at(&self, offset: usize) -> i32 {
        i32::from_le_bytes(self.bytes[offset..offset + 4].try_into().unwrap())
    }

    pub fn accumulator(&self, board: &Board, perspective: Color) -> [i16; WIDTH] {
        let mut result = self.bias;
        let (features, count) = fixed_features(board, perspective);
        for &feature in &features[..count] {
            let row = &self.weights[feature * WIDTH..(feature + 1) * WIDTH];
            for (value, &weight) in result.iter_mut().zip(row) {
                *value = value.wrapping_add(weight);
            }
        }
        result
    }

    /// All raw subnet outputs; these are not centipawn evaluations.
    pub fn raw_outputs(&self, board: &Board) -> [i32; 8] {
        let inputs = self.inputs(board);
        std::array::from_fn(|bucket| self.raw_bucket(&inputs, bucket))
    }

    /// Approximate search score in centipawns, from the side-to-move perspective.
    /// Unlike Caissa's full evaluator this excludes its endgame and castling adjustments.
    pub fn evaluate(&self, board: &Board) -> i32 {
        let white = self.accumulator(board, Color::White);
        let black = self.accumulator(board, Color::Black);
        self.evaluate_accumulators(board, &white, &black)
    }

    fn evaluate_accumulators(
        &self,
        board: &Board,
        white: &[i16; WIDTH],
        black: &[i16; WIDTH],
    ) -> i32 {
        let pieces = board.combined().popcnt().saturating_sub(2) as usize;
        let bucket = (pieces / 4).min(7);
        let (ours, theirs) = if board.side_to_move() == Color::White {
            (white, black)
        } else {
            (black, white)
        };
        let raw = self.raw_bucket(&Self::inputs_from_accumulators(ours, theirs), bucket);
        let phase = board.pieces(Piece::Knight).popcnt()
            + board.pieces(Piece::Bishop).popcnt()
            + 2 * board.pieces(Piece::Rook).popcnt()
            + 4 * board.pieces(Piece::Queen).popcnt();
        let scaled = i64::from(raw / 1506) * i64::from(52 + phase) / 64;
        scaled.clamp(-29_000, 29_000) as i32
    }

    fn inputs(&self, board: &Board) -> [u8; WIDTH] {
        let ours = self.accumulator(board, board.side_to_move());
        let theirs = self.accumulator(board, !board.side_to_move());
        Self::inputs_from_accumulators(&ours, &theirs)
    }

    fn inputs_from_accumulators(ours: &[i16; WIDTH], theirs: &[i16; WIDTH]) -> [u8; WIDTH] {
        let mut inputs = [0u8; WIDTH];
        for (perspective, acc) in [ours, theirs].iter().enumerate() {
            for i in 0..WIDTH / 2 {
                let a = i32::from(acc[i]).clamp(0, 255);
                let b = i32::from(acc[i + WIDTH / 2]).clamp(0, 255);
                inputs[perspective * WIDTH / 2 + i] = ((a * b) >> 9) as u8;
            }
        }
        inputs
    }

    fn transition(
        &self,
        values: &mut [i16; WIDTH],
        before: &Board,
        after: &Board,
        perspective: Color,
    ) {
        if feature_context(before, perspective) != feature_context(after, perspective) {
            *values = self.accumulator(after, perspective);
            return;
        }
        let (bucket, mask) = feature_context(after, perspective);
        for (color_index, color) in [perspective, !perspective].into_iter().enumerate() {
            for (piece_index, piece) in [
                Piece::Pawn,
                Piece::Knight,
                Piece::Bishop,
                Piece::Rook,
                Piece::Queen,
                Piece::King,
            ]
            .into_iter()
            .enumerate()
            {
                let old = *before.pieces(piece) & *before.color_combined(color);
                let new = *after.pieces(piece) & *after.color_combined(color);
                for (squares, add) in [(old & !new, false), (new & !old, true)] {
                    for square in squares {
                        let feature = bucket * 768
                            + (color_index * 6 + piece_index) * 64
                            + (square.to_index() ^ mask);
                        let row = &self.weights[feature * WIDTH..(feature + 1) * WIDTH];
                        for (value, &weight) in values.iter_mut().zip(row) {
                            *value = if add {
                                value.wrapping_add(weight)
                            } else {
                                value.wrapping_sub(weight)
                            };
                        }
                    }
                }
            }
        }
    }

    fn raw_bucket(&self, inputs: &[u8; WIDTH], bucket: usize) -> i32 {
        let offset = bucket * SUBNET_SIZE;
        let l1 = self.hidden::<16>(inputs, offset, offset + 24576);
        let l2 = self.hidden::<32>(&l1, offset + 24640, offset + 25152);
        let bias = self.i32_at(offset + 25344);
        l2.iter().enumerate().fold(bias, |sum, (i, &v)| {
            sum.wrapping_add(i32::from(v) * i32::from(self.i16_at(offset + 25280 + i * 2)))
        })
    }

    fn hidden<const OUTPUTS: usize>(
        &self,
        inputs: &[u8],
        weights: usize,
        biases: usize,
    ) -> [u8; OUTPUTS] {
        let mut sums: [i32; OUTPUTS] = std::array::from_fn(|i| self.i32_at(biases + i * 4));
        for (group, chunk) in inputs.as_chunks::<4>().0.iter().enumerate() {
            if chunk.iter().all(|&value| value == 0) {
                continue;
            }
            let group_offset = weights + group * OUTPUTS * 4;
            for (out, sum) in sums.iter_mut().enumerate() {
                let base = group_offset + out * 4;
                for (i, &value) in chunk.iter().enumerate() {
                    *sum =
                        sum.wrapping_add(i32::from(value) * i32::from(self.bytes[base + i] as i8));
                }
            }
        }
        sums.map(|sum| (sum >> 6).clamp(0, 127) as u8)
    }
}

fn feature_context(board: &Board, perspective: Color) -> (usize, usize) {
    let king = board.king_square(perspective).to_index();
    let mask =
        (if king % 8 >= 4 { 7 } else { 0 }) | (if perspective == Color::Black { 56 } else { 0 });
    let king = king ^ mask;
    ((king / 8) * 4 + king % 8, mask)
}

fn feature_indices(board: &Board, perspective: Color) -> Vec<usize> {
    let (features, count) = fixed_features(board, perspective);
    let mut result = features[..count].to_vec();
    result.sort_unstable();
    result
}

fn fixed_features(board: &Board, perspective: Color) -> ([usize; 32], usize) {
    let (bucket, mask) = feature_context(board, perspective);
    let mut features = [0; 32];
    let mut count = 0;
    for (color_index, color) in [perspective, !perspective].into_iter().enumerate() {
        for (piece_index, piece) in [
            Piece::Pawn,
            Piece::Knight,
            Piece::Bishop,
            Piece::Rook,
            Piece::Queen,
            Piece::King,
        ]
        .into_iter()
        .enumerate()
        {
            for square in *board.pieces(piece) & *board.color_combined(color) {
                features[count] = bucket * 768
                    + (color_index * 6 + piece_index) * 64
                    + (square.to_index() ^ mask);
                count += 1;
            }
        }
    }
    (features, count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chess::{ChessMove, MoveGen};
    use std::str::FromStr;
    fn patterned_network() -> Arc<Network> {
        let weights = (0..INPUTS * WIDTH)
            .map(|i| ((i * 13 + i / WIDTH * 7) % 31) as i16 - 15)
            .collect();
        let mut bytes = vec![0; SUBNET_SIZE * 8];
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = (i * 17 + 23) as u8;
        }
        Arc::new(Network {
            weights,
            bias: [100; WIDTH],
            bytes,
        })
    }

    fn check_stack(stack: &mut SearchEvaluator, net: &Network, board: &Board) {
        stack.ensure();
        let frame = &stack.frames[stack.ply];
        assert_eq!(frame.white, net.accumulator(board, Color::White));
        assert_eq!(frame.black, net.accumulator(board, Color::Black));
        let inputs = Network::inputs_from_accumulators(
            if board.side_to_move() == Color::White {
                &frame.white
            } else {
                &frame.black
            },
            if board.side_to_move() == Color::White {
                &frame.black
            } else {
                &frame.white
            },
        );
        assert_eq!(
            std::array::from_fn::<_, 8, _>(|b| net.raw_bucket(&inputs, b)),
            net.raw_outputs(board)
        );
        assert_eq!(stack.evaluate(board), net.evaluate(board));
    }

    #[test]
    fn lazy_stack_special_moves_and_all_promotions_restore_exact_raw_outputs() {
        let net = patterned_network();
        let mut cases = vec![
            ("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1", "e1g1"),
            ("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1", "e1c1"),
            ("r3k2r/8/8/8/8/8/8/R3K2R b KQkq - 0 1", "e8g8"),
            ("r3k2r/8/8/8/8/8/8/R3K2R b KQkq - 0 1", "e8c8"),
            ("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1", "e5d6"),
            ("4k3/8/8/8/8/8/8/3K4 w - - 0 1", "d1e1"),
        ];
        for token in ["a7a8q", "a7a8r", "a7a8b", "a7a8n"] {
            cases.push(("4k3/P7/8/8/8/8/8/4K3 w - - 0 1", token));
        }
        for (fen, token) in cases {
            let board = Board::from_str(fen).unwrap();
            let mv = ChessMove::from_str(token).unwrap();
            assert!(MoveGen::new_legal(&board).any(|m| m == mv));
            let next = board.make_move_new(mv);
            let mut stack = SearchEvaluator::new(Arc::clone(&net), &board);
            stack.push(&next);
            check_stack(&mut stack, &net, &next);
            stack.pop();
            check_stack(&mut stack, &net, &board);
        }
    }

    #[test]
    fn lazy_stack_random_paths_skipped_evaluations_and_sibling_reuse() {
        let net = patterned_network();
        let mut seed = 2026u64;
        for _ in 0..8 {
            let mut board = Board::default();
            let mut stack = SearchEvaluator::new(Arc::clone(&net), &board);
            let mut path = vec![board];
            for ply in 1..=110 {
                let moves: Vec<_> = MoveGen::new_legal(&board).collect();
                if moves.is_empty() {
                    break;
                }
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                board = board.make_move_new(moves[(seed >> 32) as usize % moves.len()]);
                path.push(board);
                stack.push(&board);
                if ply % 7 == 0 {
                    check_stack(&mut stack, &net, &board);
                }
            }
            while path.len() > 1 {
                path.pop();
                stack.pop();
                board = *path.last().unwrap();
                check_stack(&mut stack, &net, &board);
                // An unsearched or cutoff sibling must not reuse the old child's state.
                if let Some(mv) = MoveGen::new_legal(&board).next() {
                    let sibling = board.make_move_new(mv);
                    stack.push(&sibling);
                    check_stack(&mut stack, &net, &sibling);
                    stack.pop();
                }
            }
        }
    }

    #[test]
    fn validates_header_and_empty_network_output() {
        assert!(Network::from_bytes(vec![0; 64]).is_err());
        let mut invalid = vec![0; FILE_SIZE];
        assert_eq!(
            Network::from_bytes(std::mem::take(&mut invalid))
                .err()
                .unwrap(),
            "invalid network magic"
        );
        invalid = vec![0; FILE_SIZE];
        invalid[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        invalid[4..8].copy_from_slice(&(VERSION - 1).to_le_bytes());
        assert_eq!(
            Network::from_bytes(invalid).err().unwrap(),
            "unsupported network version"
        );
        let mut bytes = vec![0; FILE_SIZE];
        bytes[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        bytes[4..8].copy_from_slice(&VERSION.to_le_bytes());
        let net = Arc::new(Network::from_bytes(bytes).unwrap());
        assert_eq!(net.raw_outputs(&Board::default()), [0; 8]);
    }
    #[test]
    fn initial_features_are_in_range_and_unique() {
        for color in [Color::White, Color::Black] {
            let mut features = feature_indices(&Board::default(), color);
            assert_eq!(features.len(), 32);
            assert!(features.iter().all(|&index| index < INPUTS));
            features.sort_unstable();
            features.dedup();
            assert_eq!(features.len(), 32);
        }
    }

    #[test]
    fn sparse_hidden_layer_matches_simple_reference() {
        let mut bytes = vec![0; 128];
        for (i, value) in bytes[..48].iter_mut().enumerate() {
            *value = ((i * 37 + 11) as u8).wrapping_sub(128);
        }
        for (i, bias) in [-1000i32, 0, 3000, 9000].iter().enumerate() {
            bytes[64 + i * 4..68 + i * 4].copy_from_slice(&bias.to_le_bytes());
        }
        let net = Network {
            weights: Vec::new(),
            bias: [0; WIDTH],
            bytes,
        };
        let input = [0, 0, 0, 0, 4, 80, 127, 1, 2, 0, 64, 17];
        let actual = net.hidden::<4>(&input, 0, 64);
        let expected: [u8; 4] = std::array::from_fn(|out| {
            let mut sum = net.i32_at(64 + out * 4);
            for (i, &value) in input.iter().enumerate() {
                let index = (i / 4) * 16 + out * 4 + i % 4;
                sum = sum.wrapping_add(i32::from(value) * i32::from(net.bytes[index] as i8));
            }
            (sum >> 6).clamp(0, 127) as u8
        });
        assert_eq!(actual, expected);
    }

    #[test]
    fn incremental_features_match_refresh_for_special_moves() {
        let mut bytes = vec![0; FILE_SIZE];
        bytes[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        bytes[4..8].copy_from_slice(&VERSION.to_le_bytes());
        for feature in 0..INPUTS {
            let offset = HEADER + feature * WIDTH * 2;
            let weight = (feature as i16).wrapping_mul(13);
            bytes[offset..offset + 2].copy_from_slice(&weight.to_le_bytes());
        }
        let net = Arc::new(Network::from_bytes(bytes).unwrap());
        for (fen, token) in [
            ("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1", "e1g1"),
            ("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1", "e5d6"),
            ("4k3/P7/8/8/8/8/8/4K3 w - - 0 1", "a7a8n"),
            ("4k3/8/8/8/8/8/8/3K4 w - - 0 1", "d1e1"),
        ] {
            let before = Board::from_str(fen).unwrap();
            let mv = ChessMove::from_str(token).unwrap();
            assert!(MoveGen::new_legal(&before).any(|legal| legal == mv));
            let after = before.make_move_new(mv);
            let mut cache = CachedEvaluator::new(Arc::clone(&net), &before);
            assert_eq!(cache.evaluate(&after), net.evaluate(&after));
            assert_eq!(cache.white, net.accumulator(&after, Color::White));
            assert_eq!(cache.black, net.accumulator(&after, Color::Black));
            assert_eq!(cache.evaluate(&before), net.evaluate(&before));
            assert_eq!(cache.white, net.accumulator(&before, Color::White));
            assert_eq!(cache.black, net.accumulator(&before, Color::Black));
            for color in [Color::White, Color::Black] {
                let mut acc = PerspectiveAccumulator::new(&net, &before, color);
                acc.update(&after);
                assert_eq!(*acc.values(), net.accumulator(&after, color), "{token}");
                acc.update(&before);
                assert_eq!(
                    *acc.values(),
                    net.accumulator(&before, color),
                    "undo {token}"
                );
            }
        }
        // Search may skip whole branches, so consecutive evaluated positions
        // are not necessarily parent and child.
        let initial = Board::default();
        let mut cache = CachedEvaluator::new(Arc::clone(&net), &initial);
        for fen in [
            "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1",
            "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1",
            "4k3/P7/8/8/8/8/8/4K3 w - - 0 1",
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        ] {
            let board = Board::from_str(fen).unwrap();
            assert_eq!(cache.evaluate(&board), net.evaluate(&board));
            assert_eq!(cache.white, net.accumulator(&board, Color::White));
            assert_eq!(cache.black, net.accumulator(&board, Color::Black));
        }
    }
}
