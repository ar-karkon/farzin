//! Core value types shared by the engine: squares, pieces and packed moves.

pub type Bitboard = u64;
pub type Square = usize;

pub const WHITE: usize = 0;
pub const BLACK: usize = 1;

pub const PAWN: usize = 0;
pub const KNIGHT: usize = 1;
pub const BISHOP: usize = 2;
pub const ROOK: usize = 3;
pub const QUEEN: usize = 4;
pub const KING: usize = 5;

/// Mailbox code of an empty square. Occupied squares hold `color * 6 + piece`.
pub const NO_PIECE: u8 = 12;

pub const FILE_A: Bitboard = 0x0101_0101_0101_0101;
pub const FILE_H: Bitboard = FILE_A << 7;
pub const RANK_1: Bitboard = 0xFF;
pub const RANK_2: Bitboard = RANK_1 << 8;
pub const RANK_4: Bitboard = RANK_1 << 24;
pub const RANK_5: Bitboard = RANK_1 << 32;
pub const RANK_7: Bitboard = RANK_1 << 48;
pub const RANK_8: Bitboard = RANK_1 << 56;

#[inline(always)]
pub const fn bit(sq: Square) -> Bitboard {
    1u64 << sq
}

#[inline(always)]
pub fn lsb(bb: Bitboard) -> Square {
    bb.trailing_zeros() as Square
}

#[inline(always)]
pub fn pop_lsb(bb: &mut Bitboard) -> Square {
    let sq = lsb(*bb);
    *bb &= *bb - 1;
    sq
}

#[inline(always)]
pub const fn rank_of(sq: Square) -> usize {
    sq >> 3
}

#[inline(always)]
pub const fn file_of(sq: Square) -> usize {
    sq & 7
}

/// Mirror a square vertically (a1 <-> a8).
#[inline(always)]
pub const fn flip(sq: Square) -> Square {
    sq ^ 56
}

pub fn square_name(sq: Square) -> String {
    let file = (b'a' + file_of(sq) as u8) as char;
    let rank = (b'1' + rank_of(sq) as u8) as char;
    format!("{file}{rank}")
}

pub fn parse_square(text: &str) -> Option<Square> {
    let bytes = text.as_bytes();
    if bytes.len() != 2 || !(b'a'..=b'h').contains(&bytes[0]) || !(b'1'..=b'8').contains(&bytes[1])
    {
        return None;
    }
    Some(usize::from(bytes[1] - b'1') * 8 + usize::from(bytes[0] - b'a'))
}

/// A move packed in 16 bits: from (6) | to (6) | flag (4).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug)]
pub struct Move(pub u16);

pub const QUIET: u16 = 0;
pub const DOUBLE_PUSH: u16 = 1;
pub const KING_CASTLE: u16 = 2;
pub const QUEEN_CASTLE: u16 = 3;
pub const CAPTURE: u16 = 4;
pub const EN_PASSANT: u16 = 5;
pub const PROMO: u16 = 8;
pub const PROMO_CAPTURE: u16 = 12;

impl Move {
    pub const NONE: Move = Move(0);

    #[inline(always)]
    pub const fn new(from: Square, to: Square, flag: u16) -> Move {
        Move(from as u16 | ((to as u16) << 6) | (flag << 12))
    }
    #[inline(always)]
    pub const fn from(self) -> Square {
        (self.0 & 63) as Square
    }
    #[inline(always)]
    pub const fn to(self) -> Square {
        ((self.0 >> 6) & 63) as Square
    }
    #[inline(always)]
    pub const fn flag(self) -> u16 {
        self.0 >> 12
    }
    #[inline(always)]
    pub const fn is_none(self) -> bool {
        self.0 == 0
    }
    #[inline(always)]
    pub const fn is_capture(self) -> bool {
        self.flag() & CAPTURE != 0
    }
    #[inline(always)]
    pub const fn is_promotion(self) -> bool {
        self.flag() & PROMO != 0
    }
    /// Captures and promotions: the moves searched by quiescence.
    #[inline(always)]
    pub const fn is_noisy(self) -> bool {
        self.flag() & (CAPTURE | PROMO) != 0
    }
    #[inline(always)]
    pub const fn is_castle(self) -> bool {
        matches!(self.flag(), KING_CASTLE | QUEEN_CASTLE)
    }
    /// Promotion piece type (KNIGHT..=QUEEN) for promotion moves.
    #[inline(always)]
    pub const fn promotion(self) -> usize {
        (self.flag() & 3) as usize + 1
    }
    pub fn to_uci(self) -> String {
        if self.is_none() {
            return "0000".to_string();
        }
        let mut text = square_name(self.from()) + &square_name(self.to());
        if self.is_promotion() {
            text.push(['n', 'b', 'r', 'q'][self.promotion() - 1]);
        }
        text
    }
}
