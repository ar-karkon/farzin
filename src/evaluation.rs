use chess::{Board, Color, Piece};

pub(crate) fn evaluate(board: &Board) -> i32 {
    let pieces = [
        (Piece::Pawn, 100),
        (Piece::Knight, 320),
        (Piece::Bishop, 330),
        (Piece::Rook, 500),
        (Piece::Queen, 900),
    ];
    let mut score = 0;
    for (piece, value) in pieces {
        let white = (board.pieces(piece) & board.color_combined(Color::White)).popcnt() as i32;
        let black = (board.pieces(piece) & board.color_combined(Color::Black)).popcnt() as i32;
        score += (white - black) * value;
    }
    if board.side_to_move() == Color::White {
        score
    } else {
        -score
    }
}

/// Small handcrafted positional terms for a controlled experiment.
pub(crate) fn evaluate_positional(board: &Board) -> i32 {
    let mut white_score = 0;
    for color in [Color::White, Color::Black] {
        let sign = if color == Color::White { 1 } else { -1 };
        for piece in [
            Piece::Pawn,
            Piece::Knight,
            Piece::Bishop,
            Piece::Rook,
            Piece::Queen,
        ] {
            for square in *board.pieces(piece) & *board.color_combined(color) {
                let file = square.get_file().to_index() as i32;
                let rank = square.get_rank().to_index() as i32;
                let center = 7 - (2 * file - 7).abs() - (2 * rank - 7).abs();
                let advance = if color == Color::White {
                    rank
                } else {
                    7 - rank
                };
                let bonus = match piece {
                    Piece::Pawn => 4 * advance,
                    Piece::Knight => 4 * center,
                    Piece::Bishop => 2 * center,
                    Piece::Rook | Piece::Queen => center,
                    Piece::King => 0,
                };
                white_score += sign * bonus;
            }
        }
    }
    evaluate(board)
        + if board.side_to_move() == Color::White {
            white_score
        } else {
            -white_score
        }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn positional_terms_are_symmetric_and_reward_central_knights() {
        let symmetric = Board::from_str("4k3/8/8/3n4/3N4/8/8/4K3 w - - 0 1").unwrap();
        assert_eq!(evaluate_positional(&symmetric), 0);
        let center = Board::from_str("7k/8/8/8/3N4/8/8/K7 w - - 0 1").unwrap();
        let edge = Board::from_str("7k/8/8/8/N7/8/8/K7 w - - 0 1").unwrap();
        assert!(evaluate_positional(&center) > evaluate_positional(&edge));
    }
}
