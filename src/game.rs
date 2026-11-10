use chess::{Board, BoardBuilder, ChessMove, MoveGen, Piece};
use std::str::FromStr;

#[derive(Clone)]
pub(crate) struct Game {
    pub(crate) board: Board,
    pub(crate) history: Vec<u64>,
    pub(crate) halfmove: u32,
    /// Index in `history` of the first position that may be repeated. A null
    /// move is a repetition barrier: positions before it are never counted.
    pub(crate) barrier: usize,
}

impl Game {
    pub(crate) fn new(board: Board, halfmove: u32) -> Self {
        Self {
            board,
            history: vec![repetition_hash(&board)],
            halfmove,
            barrier: 0,
        }
    }
    pub(crate) fn play(&mut self, mv: ChessMove) {
        let pawn = self.board.piece_on(mv.get_source()) == Some(Piece::Pawn);
        let capture = self.board.piece_on(mv.get_dest()).is_some()
            || (pawn && mv.get_source().get_file() != mv.get_dest().get_file());
        self.halfmove = if pawn || capture {
            0
        } else {
            self.halfmove.saturating_add(1)
        };
        self.board = self.board.make_move_new(mv);
        self.history.push(repetition_hash(&self.board));
    }
    pub(crate) fn unplay(&mut self, board: Board, halfmove: u32) {
        self.board = board;
        self.halfmove = halfmove;
        self.history.pop();
    }
    /// Pass the move. Returns the state that `unplay_null` restores.
    pub(crate) fn play_null(&mut self, board: Board) -> (u32, usize) {
        let saved = (self.halfmove, self.barrier);
        self.board = board;
        self.halfmove = self.halfmove.saturating_add(1);
        self.history.push(repetition_hash(&self.board));
        self.barrier = self.history.len() - 1;
        saved
    }
    pub(crate) fn unplay_null(&mut self, board: Board, saved: (u32, usize)) {
        self.board = board;
        (self.halfmove, self.barrier) = saved;
        self.history.pop();
    }
    pub(crate) fn draw(&self) -> bool {
        self.halfmove >= 100
            || self.insufficient_material()
            || self.history[self.barrier..]
                .iter()
                .filter(|&&hash| Some(&hash) == self.history.last())
                .count()
                >= 3
    }

    fn insufficient_material(&self) -> bool {
        if [Piece::Pawn, Piece::Rook, Piece::Queen]
            .iter()
            .any(|&p| self.board.pieces(p).popcnt() != 0)
        {
            return false;
        }
        let knights = self.board.pieces(Piece::Knight).popcnt();
        let bishops = *self.board.pieces(Piece::Bishop);
        if knights + bishops.popcnt() <= 1 {
            return true;
        }
        if knights != 0 {
            return false;
        }
        let mut colors = bishops
            .into_iter()
            .map(|sq| (sq.get_rank().to_index() + sq.get_file().to_index()) % 2);
        let first = colors.next();
        colors.all(|color| Some(color) == first)
    }
}

// FIDE repetition distinguishes en passant only when a legal capture exists.
// The chess crate retains an EP hash for pseudo-legal captures by pinned pawns.
fn repetition_hash(board: &Board) -> u64 {
    if board.en_passant().is_none() {
        return board.get_hash();
    }
    let legal_en_passant = MoveGen::new_legal(board).any(|mv| {
        board.piece_on(mv.get_source()) == Some(Piece::Pawn)
            && mv.get_source().get_file() != mv.get_dest().get_file()
            && board.piece_on(mv.get_dest()).is_none()
    });
    if legal_en_passant {
        return board.get_hash();
    }
    let mut builder = BoardBuilder::from(board);
    builder.en_passant(None);
    Board::try_from(&builder)
        .expect("removing en passant preserves a legal board")
        .get_hash()
}

pub(crate) fn position(command: &str) -> Result<Game, String> {
    let mut parts = command.split_whitespace();
    let mut game = match parts.next() {
        Some("startpos") => Game::new(Board::default(), 0),
        Some("fen") => {
            let fen: Vec<_> = parts.by_ref().take(6).collect();
            if fen.len() != 6 {
                return Err("FEN requires six fields".into());
            }
            let halfmove = fen[4].parse().map_err(|_| "invalid halfmove clock")?;
            let board = Board::from_str(&fen.join(" ")).map_err(|e| format!("invalid FEN: {e}"))?;
            Game::new(board, halfmove)
        }
        _ => return Err("expected startpos or fen".into()),
    };
    if let Some(token) = parts.next() {
        if token != "moves" {
            return Err("expected moves".into());
        }
        for token in parts {
            let mv = ChessMove::from_str(token).map_err(|_| format!("invalid move: {token}"))?;
            if !MoveGen::new_legal(&game.board).any(|legal| legal == mv) {
                return Err(format!("illegal move: {token}"));
            }
            game.play(mv);
        }
    }
    Ok(game)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_dead_material_is_drawn() {
        for fen in [
            "7k/8/8/8/8/8/8/4K3 w - - 0 1",
            "7k/8/8/8/8/8/8/2B1K3 w - - 0 1",
            "7k/8/8/8/8/8/8/2N1K3 w - - 0 1",
        ] {
            assert!(position(&format!("fen {fen}")).unwrap().draw());
        }
        assert!(!position("fen 7k/8/8/8/8/8/8/1BN1K3 w - - 0 1")
            .unwrap()
            .draw());
    }

    #[test]
    fn repetition_ignores_pinned_en_passant_but_preserves_legal_right() {
        let pinned = position("fen k3r3/8/8/3pP3/8/8/8/4K3 w - d6 0 1").unwrap();
        let no_right = position("fen k3r3/8/8/3pP3/8/8/8/4K3 w - - 0 1").unwrap();
        assert_ne!(pinned.board.get_hash(), no_right.board.get_hash());
        assert_eq!(pinned.history, no_right.history);

        let legal = position("fen k7/8/8/3pP3/8/8/8/4K3 w - d6 0 1").unwrap();
        let no_right = position("fen k7/8/8/3pP3/8/8/8/4K3 w - - 0 1").unwrap();
        assert_ne!(legal.history, no_right.history);

        let repeated = position(
            "fen k3r3/8/8/3pP3/8/8/8/4K3 w - d6 0 1 moves e1f1 a8b8 f1e1 b8a8 e1f1 a8b8 f1e1 b8a8",
        )
        .unwrap();
        assert!(repeated.draw());
    }
}
