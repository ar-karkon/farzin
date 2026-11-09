//! Experimental Caissa v15 scalar inference; not connected to the engine.
//!
//! File layout and mathematics refer to Caissa commit 395c2e6 by Michał
//! Witanowski (MIT); see THIRD_PARTY_NOTICES.md. Weights are not distributed.
use chess::{Board, Color, Piece};
use std::path::Path;

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
    bytes: Vec<u8>,
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
        let row = HEADER + feature * WIDTH * 2;
        for (i, value) in self.values.iter_mut().enumerate() {
            let weight = self.network.i16_at(row + i * 2);
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
        Ok(Self { bytes })
    }

    fn i16_at(&self, offset: usize) -> i16 {
        i16::from_le_bytes([self.bytes[offset], self.bytes[offset + 1]])
    }
    fn i32_at(&self, offset: usize) -> i32 {
        i32::from_le_bytes(self.bytes[offset..offset + 4].try_into().unwrap())
    }

    pub fn accumulator(&self, board: &Board, perspective: Color) -> [i16; WIDTH] {
        let mut result = [0i16; WIDTH];
        for (i, value) in result.iter_mut().enumerate() {
            *value = self.i16_at(BIAS_OFFSET + i * 2);
        }
        for feature in feature_indices(board, perspective) {
            let row = HEADER + feature * WIDTH * 2;
            for (i, value) in result.iter_mut().enumerate() {
                *value = value.wrapping_add(self.i16_at(row + i * 2));
            }
        }
        result
    }

    /// All raw subnet outputs; these are not centipawn evaluations.
    pub fn raw_outputs(&self, board: &Board) -> [i32; 8] {
        let ours = self.accumulator(board, board.side_to_move());
        let theirs = self.accumulator(board, !board.side_to_move());
        let mut inputs = [0u8; WIDTH];
        for (perspective, acc) in [ours, theirs].iter().enumerate() {
            for i in 0..WIDTH / 2 {
                let a = i32::from(acc[i]).clamp(0, 255);
                let b = i32::from(acc[i + WIDTH / 2]).clamp(0, 255);
                inputs[perspective * WIDTH / 2 + i] = ((a * b) >> 9) as u8;
            }
        }
        std::array::from_fn(|bucket| {
            let offset = SUBNET_OFFSET + bucket * SUBNET_SIZE;
            let l1 = self.hidden(&inputs, offset, offset + 24576, 16);
            let l2 = self.hidden(&l1, offset + 24640, offset + 25152, 32);
            let bias = self.i32_at(offset + 25344);
            l2.iter().enumerate().fold(bias, |sum, (i, &v)| {
                sum.wrapping_add(i32::from(v) * i32::from(self.i16_at(offset + 25280 + i * 2)))
            })
        })
    }

    fn hidden(&self, inputs: &[u8], weights: usize, biases: usize, outputs: usize) -> Vec<u8> {
        (0..outputs)
            .map(|out| {
                let mut sum = self.i32_at(biases + out * 4);
                for (i, &value) in inputs.iter().enumerate() {
                    let index = (i / 4) * (outputs * 4) + out * 4 + i % 4;
                    sum = sum.wrapping_add(
                        i32::from(value) * i32::from(self.bytes[weights + index] as i8),
                    );
                }
                (sum >> 6).clamp(0, 127) as u8
            })
            .collect()
    }
}

fn feature_indices(board: &Board, perspective: Color) -> Vec<usize> {
    let king = board.king_square(perspective).to_index();
    let mask =
        (if king % 8 >= 4 { 7 } else { 0 }) | (if perspective == Color::Black { 56 } else { 0 });
    let king = king ^ mask;
    let bucket = (king / 8) * 4 + king % 8;
    let mut features = Vec::with_capacity(32);
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
                features.push(
                    bucket * 768
                        + (color_index * 6 + piece_index) * 64
                        + (square.to_index() ^ mask),
                );
            }
        }
    }
    features.sort_unstable();
    features
}

#[cfg(test)]
mod tests {
    use super::*;
    use chess::{ChessMove, MoveGen};
    use std::str::FromStr;
    #[test]
    fn validates_header_and_empty_network_output() {
        assert!(Network::from_bytes(vec![0; 64]).is_err());
        let mut bytes = vec![0; FILE_SIZE];
        bytes[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        bytes[4..8].copy_from_slice(&VERSION.to_le_bytes());
        let net = Network::from_bytes(bytes).unwrap();
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
    fn incremental_features_match_refresh_for_special_moves() {
        let mut bytes = vec![0; FILE_SIZE];
        bytes[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        bytes[4..8].copy_from_slice(&VERSION.to_le_bytes());
        for feature in 0..INPUTS {
            let offset = HEADER + feature * WIDTH * 2;
            let weight = (feature as i16).wrapping_mul(13);
            bytes[offset..offset + 2].copy_from_slice(&weight.to_le_bytes());
        }
        let net = Network::from_bytes(bytes).unwrap();
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
    }
}
