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
