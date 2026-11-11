//! Rebuild a Board from a Farzin training record.
use farzin::board::Board;
use farzin::datagen::unpack;
use farzin::types::*;

pub fn board_from(rec: &[u8]) -> Board {
    let r = unpack(rec);
    let mut fen = String::new();
    let mut grid = [12usize; 64];
    for &(code, sq) in &r.pieces {
        grid[sq] = code;
    }
    for rank in (0..8).rev() {
        let mut empty = 0;
        for file in 0..8 {
            let c = grid[rank * 8 + file];
            if c == 12 {
                empty += 1;
            } else {
                if empty > 0 {
                    fen.push_str(&empty.to_string());
                    empty = 0;
                }
                fen.push(b"PNBRQKpnbrqk"[c] as char);
            }
        }
        if empty > 0 {
            fen.push_str(&empty.to_string());
        }
        if rank > 0 {
            fen.push('/');
        }
    }
    fen.push_str(if r.stm == WHITE {
        " w - - 0 1"
    } else {
        " b - - 0 1"
    });
    Board::from_fen(&fen).expect("valid record")
}
