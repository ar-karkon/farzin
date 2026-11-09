use chess::{Board, ChessMove, MoveGen, Piece};
use std::str::FromStr;

#[derive(Clone)]
pub(crate) struct Game {
    pub(crate) board: Board,
    pub(crate) history: Vec<u64>,
    pub(crate) halfmove: u32,
}

impl Game {
    pub(crate) fn new(board: Board, halfmove: u32) -> Self {
        Self {
            board,
            history: vec![board.get_hash()],
            halfmove,
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
        self.history.push(self.board.get_hash());
    }
    pub(crate) fn unplay(&mut self, board: Board, halfmove: u32) {
        self.board = board;
        self.halfmove = halfmove;
        self.history.pop();
    }
    pub(crate) fn draw(&self) -> bool {
        self.halfmove >= 100
            || self.insufficient_material()
            || self
                .history
                .iter()
                .filter(|&&hash| hash == self.board.get_hash())
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
}
