//! Scalar accumulator prototype for piece-square features.
//!
//! This feature layout is deliberately simple and is **not** compatible with
//! Caissa, Stockfish, or any supplied network. No trained model is provided.
use chess::{Board, Color, Piece};

pub const FEATURE_COUNT: usize = 12 * 64;

/// Return sorted, unique indices for absolute-color piece-square features.
pub fn piece_square_features(board: &Board) -> Vec<usize> {
    let mut features = Vec::with_capacity(32);
    let pieces = [
        Piece::Pawn,
        Piece::Knight,
        Piece::Bishop,
        Piece::Rook,
        Piece::Queen,
        Piece::King,
    ];
    for (color_index, color) in [Color::White, Color::Black].into_iter().enumerate() {
        for (piece_index, piece) in pieces.into_iter().enumerate() {
            for square in *board.pieces(piece) & *board.color_combined(color) {
                features.push((color_index * 6 + piece_index) * 64 + square.to_index());
            }
        }
    }
    features.sort_unstable();
    features
}

/// Row-major feature weights and bias for one scalar reference accumulator.
pub struct Transformer {
    width: usize,
    weights: Vec<i16>,
    bias: Vec<i32>,
}

impl Transformer {
    pub fn new(width: usize, weights: Vec<i16>, bias: Vec<i32>) -> Result<Self, &'static str> {
        if width == 0
            || bias.len() != width
            || FEATURE_COUNT.checked_mul(width) != Some(weights.len())
        {
            return Err("invalid transformer dimensions");
        }
        Ok(Self {
            width,
            weights,
            bias,
        })
    }

    pub fn refresh(&self, board: &Board) -> Result<Accumulator, &'static str> {
        let mut accumulator = Accumulator {
            values: self.bias.clone(),
        };
        accumulator.update(self, &[], &piece_square_features(board))?;
        Ok(accumulator)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Accumulator {
    values: Vec<i32>,
}

impl Accumulator {
    pub fn values(&self) -> &[i32] {
        &self.values
    }

    /// Update transactionally; invalid feature indices or overflow leave the original untouched.
    pub fn update(
        &mut self,
        transformer: &Transformer,
        removed: &[usize],
        added: &[usize],
    ) -> Result<(), &'static str> {
        if self.values.len() != transformer.width {
            return Err("incompatible accumulator width");
        }
        let mut result = self.values.clone();
        for (indices, sign) in [(removed, -1_i32), (added, 1_i32)] {
            for &feature in indices {
                if feature >= FEATURE_COUNT {
                    return Err("feature index out of range");
                }
                let row = &transformer.weights
                    [feature * transformer.width..(feature + 1) * transformer.width];
                for (value, &weight) in result.iter_mut().zip(row) {
                    *value = value
                        .checked_add(sign * i32::from(weight))
                        .ok_or("accumulator overflow")?;
                }
            }
        }
        self.values = result;
        Ok(())
    }

    pub fn transition(
        &mut self,
        transformer: &Transformer,
        before: &Board,
        after: &Board,
    ) -> Result<(), &'static str> {
        let old = piece_square_features(before);
        let new = piece_square_features(after);
        let removed: Vec<_> = old
            .iter()
            .copied()
            .filter(|f| new.binary_search(f).is_err())
            .collect();
        let added: Vec<_> = new
            .iter()
            .copied()
            .filter(|f| old.binary_search(f).is_err())
            .collect();
        self.update(transformer, &removed, &added)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chess::{ChessMove, MoveGen};
    use std::str::FromStr;

    fn transformer() -> Transformer {
        let width = 8;
        let weights = (0..FEATURE_COUNT * width)
            .map(|i| (i as i32 % 31 - 15) as i16)
            .collect();
        Transformer::new(width, weights, vec![10; width]).unwrap()
    }

    #[test]
    fn incremental_matches_refresh_and_undo_for_special_moves() {
        for (fen, token) in [
            ("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1", "e1g1"),
            ("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1", "e5d6"),
            ("4k3/P7/8/8/8/8/8/4K3 w - - 0 1", "a7a8n"),
            (
                "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
                "e2e4",
            ),
        ] {
            let before = Board::from_str(fen).unwrap();
            let mv = ChessMove::from_str(token).unwrap();
            assert!(MoveGen::new_legal(&before).any(|legal| legal == mv));
            let after = before.make_move_new(mv);
            let model = transformer();
            let mut incremental = model.refresh(&before).unwrap();
            incremental.transition(&model, &before, &after).unwrap();
            assert_eq!(incremental, model.refresh(&after).unwrap());
            incremental.transition(&model, &after, &before).unwrap();
            assert_eq!(incremental, model.refresh(&before).unwrap());
        }
    }

    #[test]
    fn invalid_update_is_transactional() {
        let model = transformer();
        let mut acc = model.refresh(&Board::default()).unwrap();
        let original = acc.clone();
        assert!(acc.update(&model, &[0], &[FEATURE_COUNT]).is_err());
        assert_eq!(acc, original);
        assert!(Transformer::new(1, vec![0], vec![0]).is_err());
        let overflowing = Transformer::new(1, vec![1; FEATURE_COUNT], vec![i32::MAX]).unwrap();
        let mut acc = Accumulator {
            values: vec![i32::MAX],
        };
        assert!(acc.update(&overflowing, &[], &[0]).is_err());
        assert_eq!(acc.values(), &[i32::MAX]);
    }

    #[test]
    fn incremental_matches_refresh_across_a_deterministic_game() {
        let model = transformer();
        let mut board = Board::default();
        let mut acc = model.refresh(&board).unwrap();
        for ply in 0..100 {
            let moves: Vec<_> = MoveGen::new_legal(&board).collect();
            if moves.is_empty() {
                break;
            }
            let next = board.make_move_new(moves[(ply * 17 + 3) % moves.len()]);
            acc.transition(&model, &board, &next).unwrap();
            assert_eq!(acc, model.refresh(&next).unwrap(), "ply {ply}");
            board = next;
        }
    }
}
