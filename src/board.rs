//! Bitboard position with copy-make, Zobrist hashing, legality and SEE.
use crate::attacks::*;
use crate::types::*;

const fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

pub struct Zobrist {
    pub pieces: [[u64; 64]; 12],
    pub castling: [u64; 16],
    pub ep: [u64; 8],
    pub side: u64,
}

const fn make_zobrist() -> Zobrist {
    let mut state = 0x0F4B_1A2C_3D5E_6F70u64;
    let mut pieces = [[0u64; 64]; 12];
    let mut p = 0;
    while p < 12 {
        let mut s = 0;
        while s < 64 {
            pieces[p][s] = splitmix(&mut state);
            s += 1;
        }
        p += 1;
    }
    let mut castling = [0u64; 16];
    let mut c = 0;
    while c < 16 {
        castling[c] = splitmix(&mut state);
        c += 1;
    }
    let mut ep = [0u64; 8];
    let mut f = 0;
    while f < 8 {
        ep[f] = splitmix(&mut state);
        f += 1;
    }
    Zobrist {
        pieces,
        castling,
        ep,
        side: splitmix(&mut state),
    }
}

pub static ZOBRIST: Zobrist = make_zobrist();

pub const WK_CASTLE: u8 = 1;
pub const WQ_CASTLE: u8 = 2;
pub const BK_CASTLE: u8 = 4;
pub const BQ_CASTLE: u8 = 8;

const fn castle_masks() -> [u8; 64] {
    let mut masks = [15u8; 64];
    masks[0] = 15 & !WQ_CASTLE;
    masks[7] = 15 & !WK_CASTLE;
    masks[4] = 15 & !(WK_CASTLE | WQ_CASTLE);
    masks[56] = 15 & !BQ_CASTLE;
    masks[63] = 15 & !BK_CASTLE;
    masks[60] = 15 & !(BK_CASTLE | BQ_CASTLE);
    masks
}
static CASTLE_MASK: [u8; 64] = castle_masks();

pub const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Board {
    pub pieces: [Bitboard; 6],
    pub colors: [Bitboard; 2],
    pub mailbox: [u8; 64],
    pub stm: usize,
    pub castling: u8,
    /// En passant target square, or 64. Only set when a capture is pseudo-legal.
    pub ep: u8,
    pub halfmove: u16,
    pub fullmove: u16,
    pub hash: u64,
    pub pawn_hash: u64,
    /// Hash of non-pawn pieces per color (for correction history).
    pub non_pawn_hash: [u64; 2],
    pub checkers: Bitboard,
    pub pinned: Bitboard,
}

impl Board {
    pub fn startpos() -> Board {
        Board::from_fen(START_FEN).unwrap()
    }

    fn empty() -> Board {
        Board {
            pieces: [0; 6],
            colors: [0; 2],
            mailbox: [NO_PIECE; 64],
            stm: WHITE,
            castling: 0,
            ep: 64,
            halfmove: 0,
            fullmove: 1,
            hash: 0,
            pawn_hash: 0,
            non_pawn_hash: [0; 2],
            checkers: 0,
            pinned: 0,
        }
    }

    pub fn from_fen(fen: &str) -> Result<Board, String> {
        let fields: Vec<&str> = fen.split_whitespace().collect();
        if fields.len() < 4 {
            return Err(format!("FEN needs at least 4 fields: {fen}"));
        }
        let mut b = Board::empty();
        let mut rank = 7i32;
        let mut file = 0i32;
        for c in fields[0].chars() {
            match c {
                '/' => {
                    if file != 8 {
                        return Err("bad FEN rank length".into());
                    }
                    rank -= 1;
                    file = 0;
                }
                '1'..='8' => file += c as i32 - '0' as i32,
                _ => {
                    let piece = match c.to_ascii_lowercase() {
                        'p' => PAWN,
                        'n' => KNIGHT,
                        'b' => BISHOP,
                        'r' => ROOK,
                        'q' => QUEEN,
                        'k' => KING,
                        _ => return Err(format!("bad FEN piece {c}")),
                    };
                    let color = if c.is_ascii_uppercase() { WHITE } else { BLACK };
                    if !(0..8).contains(&rank) || !(0..8).contains(&file) {
                        return Err("FEN square out of range".into());
                    }
                    b.put(color, piece, (rank * 8 + file) as usize);
                    file += 1;
                }
            }
            if file > 8 {
                return Err("bad FEN rank length".into());
            }
        }
        if rank != 0 || file != 8 {
            return Err("bad FEN board".into());
        }
        b.stm = match fields[1] {
            "w" => WHITE,
            "b" => BLACK,
            _ => return Err("bad side to move".into()),
        };
        for c in fields[2].chars() {
            b.castling |= match c {
                'K' => WK_CASTLE,
                'Q' => WQ_CASTLE,
                'k' => BK_CASTLE,
                'q' => BQ_CASTLE,
                '-' => 0,
                _ => return Err("bad castling field".into()),
            };
        }
        // Drop rights that do not match the king/rook placement.
        let has = |b: &Board, c: usize, p: usize, sq: usize| b.mailbox[sq] == (c * 6 + p) as u8;
        if !(has(&b, WHITE, KING, 4) && has(&b, WHITE, ROOK, 7)) {
            b.castling &= !WK_CASTLE;
        }
        if !(has(&b, WHITE, KING, 4) && has(&b, WHITE, ROOK, 0)) {
            b.castling &= !WQ_CASTLE;
        }
        if !(has(&b, BLACK, KING, 60) && has(&b, BLACK, ROOK, 63)) {
            b.castling &= !BK_CASTLE;
        }
        if !(has(&b, BLACK, KING, 60) && has(&b, BLACK, ROOK, 56)) {
            b.castling &= !BQ_CASTLE;
        }
        if fields[3] != "-" {
            let sq = parse_square(fields[3]).ok_or("bad en passant square")?;
            let them = b.stm ^ 1;
            let our_pawns = b.pieces[PAWN] & b.colors[b.stm];
            if PAWN_ATTACKS[them][sq] & our_pawns != 0 {
                b.ep = sq as u8;
            }
        }
        b.halfmove = fields.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
        b.fullmove = fields.get(5).and_then(|s| s.parse().ok()).unwrap_or(1);
        for color in [WHITE, BLACK] {
            if (b.pieces[KING] & b.colors[color]).count_ones() != 1 {
                return Err("each side needs exactly one king".into());
            }
        }
        if b.attackers_to(b.king_sq(b.stm ^ 1), b.occupied()) & b.colors[b.stm] != 0 {
            return Err("side not to move is in check".into());
        }
        b.hash = b.compute_hash();
        b.update_masks();
        Ok(b)
    }

    pub fn to_fen(&self) -> String {
        let mut out = String::new();
        for rank in (0..8).rev() {
            let mut empty = 0;
            for file in 0..8 {
                let code = self.mailbox[rank * 8 + file];
                if code == NO_PIECE {
                    empty += 1;
                    continue;
                }
                if empty > 0 {
                    out.push((b'0' + empty) as char);
                    empty = 0;
                }
                let c = b"pnbrqk"[code as usize % 6] as char;
                out.push(if code < 6 { c.to_ascii_uppercase() } else { c });
            }
            if empty > 0 {
                out.push((b'0' + empty) as char);
            }
            if rank > 0 {
                out.push('/');
            }
        }
        out.push_str(if self.stm == WHITE { " w " } else { " b " });
        if self.castling == 0 {
            out.push('-');
        }
        for (flag, c) in [
            (WK_CASTLE, 'K'),
            (WQ_CASTLE, 'Q'),
            (BK_CASTLE, 'k'),
            (BQ_CASTLE, 'q'),
        ] {
            if self.castling & flag != 0 {
                out.push(c);
            }
        }
        out.push(' ');
        if self.ep == 64 {
            out.push('-');
        } else {
            out.push_str(&square_name(self.ep as usize));
        }
        out.push_str(&format!(" {} {}", self.halfmove, self.fullmove));
        out
    }

    fn compute_hash(&self) -> u64 {
        let mut h = 0;
        for sq in 0..64 {
            let code = self.mailbox[sq];
            if code != NO_PIECE {
                h ^= ZOBRIST.pieces[code as usize][sq];
            }
        }
        h ^= ZOBRIST.castling[self.castling as usize];
        if self.ep != 64 {
            h ^= ZOBRIST.ep[file_of(self.ep as usize)];
        }
        if self.stm == BLACK {
            h ^= ZOBRIST.side;
        }
        h
    }

    #[inline(always)]
    fn put(&mut self, color: usize, piece: usize, sq: Square) {
        let b = bit(sq);
        self.pieces[piece] |= b;
        self.colors[color] |= b;
        let code = color * 6 + piece;
        self.mailbox[sq] = code as u8;
        let key = ZOBRIST.pieces[code][sq];
        if piece == PAWN {
            self.pawn_hash ^= key;
        } else {
            self.non_pawn_hash[color] ^= key;
        }
    }

    #[inline(always)]
    fn remove(&mut self, color: usize, piece: usize, sq: Square) {
        let b = bit(sq);
        self.pieces[piece] ^= b;
        self.colors[color] ^= b;
        self.mailbox[sq] = NO_PIECE;
        let key = ZOBRIST.pieces[color * 6 + piece][sq];
        if piece == PAWN {
            self.pawn_hash ^= key;
        } else {
            self.non_pawn_hash[color] ^= key;
        }
    }

    #[inline(always)]
    pub fn occupied(&self) -> Bitboard {
        self.colors[0] | self.colors[1]
    }
    #[inline(always)]
    pub fn piece_bb(&self, color: usize, piece: usize) -> Bitboard {
        self.pieces[piece] & self.colors[color]
    }
    #[inline(always)]
    pub fn king_sq(&self, color: usize) -> Square {
        lsb(self.piece_bb(color, KING))
    }
    #[inline(always)]
    pub fn piece_on(&self, sq: Square) -> Option<usize> {
        let code = self.mailbox[sq];
        (code != NO_PIECE).then_some(code as usize % 6)
    }
    #[inline(always)]
    pub fn in_check(&self) -> bool {
        self.checkers != 0
    }

    #[inline]
    pub fn attackers_to(&self, sq: Square, occ: Bitboard) -> Bitboard {
        (PAWN_ATTACKS[WHITE][sq] & self.piece_bb(BLACK, PAWN))
            | (PAWN_ATTACKS[BLACK][sq] & self.piece_bb(WHITE, PAWN))
            | (KNIGHT_ATTACKS[sq] & self.pieces[KNIGHT])
            | (KING_ATTACKS[sq] & self.pieces[KING])
            | (bishop_attacks(sq, occ) & (self.pieces[BISHOP] | self.pieces[QUEEN]))
            | (rook_attacks(sq, occ) & (self.pieces[ROOK] | self.pieces[QUEEN]))
    }

    #[inline]
    pub fn is_attacked(&self, sq: Square, by: usize, occ: Bitboard) -> bool {
        let them = self.colors[by];
        (PAWN_ATTACKS[by ^ 1][sq] & self.pieces[PAWN] & them) != 0
            || (KNIGHT_ATTACKS[sq] & self.pieces[KNIGHT] & them) != 0
            || (KING_ATTACKS[sq] & self.pieces[KING] & them) != 0
            || (bishop_attacks(sq, occ) & (self.pieces[BISHOP] | self.pieces[QUEEN]) & them) != 0
            || (rook_attacks(sq, occ) & (self.pieces[ROOK] | self.pieces[QUEEN]) & them) != 0
    }

    /// Recompute checkers and pinned pieces for the side to move.
    fn update_masks(&mut self) {
        let us = self.stm;
        let them = us ^ 1;
        let king = self.king_sq(us);
        let occ = self.occupied();
        self.checkers = self.attackers_to(king, occ) & self.colors[them];
        let mut pinned = 0;
        let snipers = ((rook_attacks(king, 0) & (self.pieces[ROOK] | self.pieces[QUEEN]))
            | (bishop_attacks(king, 0) & (self.pieces[BISHOP] | self.pieces[QUEEN])))
            & self.colors[them];
        let mut s = snipers;
        while s != 0 {
            let sq = pop_lsb(&mut s);
            let blockers = BETWEEN[king][sq] & occ;
            if blockers.count_ones() == 1 {
                pinned |= blockers & self.colors[us];
            }
        }
        self.pinned = pinned;
    }

    /// Copy-make. `mv` must be pseudo-legal and legal for this position.
    pub fn make_move(&self, mv: Move) -> Board {
        let mut b = *self;
        let us = self.stm;
        let them = us ^ 1;
        let from = mv.from();
        let to = mv.to();
        let flag = mv.flag();
        let piece = self.mailbox[from] as usize % 6;
        b.hash ^= ZOBRIST.castling[b.castling as usize];
        if b.ep != 64 {
            b.hash ^= ZOBRIST.ep[file_of(b.ep as usize)];
            b.ep = 64;
        }
        b.halfmove += 1;
        if flag == EN_PASSANT {
            let cap = to ^ 8;
            b.remove(them, PAWN, cap);
            b.hash ^= ZOBRIST.pieces[them * 6 + PAWN][cap];
        } else if mv.is_capture() {
            let victim = self.mailbox[to] as usize % 6;
            b.remove(them, victim, to);
            b.hash ^= ZOBRIST.pieces[them * 6 + victim][to];
        }
        if mv.is_capture() || piece == PAWN {
            b.halfmove = 0;
        }
        b.remove(us, piece, from);
        b.hash ^= ZOBRIST.pieces[us * 6 + piece][from];
        let placed = if mv.is_promotion() {
            mv.promotion()
        } else {
            piece
        };
        b.put(us, placed, to);
        b.hash ^= ZOBRIST.pieces[us * 6 + placed][to];
        match flag {
            DOUBLE_PUSH => {
                let ep = (from + to) / 2;
                if PAWN_ATTACKS[us][ep] & b.piece_bb(them, PAWN) != 0 {
                    b.ep = ep as u8;
                    b.hash ^= ZOBRIST.ep[file_of(ep)];
                }
            }
            KING_CASTLE | QUEEN_CASTLE => {
                let (rf, rt) = if flag == KING_CASTLE {
                    (to + 1, to - 1)
                } else {
                    (to - 2, to + 1)
                };
                b.remove(us, ROOK, rf);
                b.put(us, ROOK, rt);
                b.hash ^= ZOBRIST.pieces[us * 6 + ROOK][rf] ^ ZOBRIST.pieces[us * 6 + ROOK][rt];
            }
            _ => {}
        }
        b.castling &= CASTLE_MASK[from] & CASTLE_MASK[to];
        b.hash ^= ZOBRIST.castling[b.castling as usize];
        b.stm = them;
        b.hash ^= ZOBRIST.side;
        if us == BLACK {
            b.fullmove += 1;
        }
        b.update_masks();
        b
    }

    pub fn make_null(&self) -> Board {
        let mut b = *self;
        if b.ep != 64 {
            b.hash ^= ZOBRIST.ep[file_of(b.ep as usize)];
            b.ep = 64;
        }
        b.halfmove += 1;
        b.stm ^= 1;
        b.hash ^= ZOBRIST.side;
        b.update_masks();
        b
    }

    /// Hash of the position after `mv`, without making it (for TT prefetch).
    /// Ignores castling/en-passant changes, so it is only an approximation.
    #[inline]
    pub fn key_after(&self, mv: Move) -> u64 {
        let from = mv.from();
        let to = mv.to();
        let code = self.mailbox[from] as usize;
        let mut h = self.hash ^ ZOBRIST.side;
        h ^= ZOBRIST.pieces[code][from] ^ ZOBRIST.pieces[code][to];
        let victim = self.mailbox[to];
        if victim != NO_PIECE {
            h ^= ZOBRIST.pieces[victim as usize][to];
        }
        h
    }

    /// Full legality test for a pseudo-legal move.
    #[inline]
    pub fn is_legal(&self, mv: Move) -> bool {
        let us = self.stm;
        let them = us ^ 1;
        let from = mv.from();
        let to = mv.to();
        let king = self.king_sq(us);
        if from == king {
            if mv.is_castle() {
                return true; // fully verified during generation / pseudo-legal check
            }
            return !self.is_attacked(to, them, self.occupied() ^ bit(from));
        }
        if mv.flag() == EN_PASSANT {
            let cap = to ^ 8;
            let occ = (self.occupied() ^ bit(from) ^ bit(cap)) | bit(to);
            let enemy = self.colors[them] & !bit(cap);
            let sliders = (rook_attacks(king, occ) & (self.pieces[ROOK] | self.pieces[QUEEN])
                | bishop_attacks(king, occ) & (self.pieces[BISHOP] | self.pieces[QUEEN]))
                & enemy;
            let leapers = self.checkers & !bit(cap) & (self.pieces[KNIGHT] | self.pieces[PAWN]);
            return sliders == 0 && leapers == 0;
        }
        if self.pinned & bit(from) != 0 && LINE[from][king] & bit(to) == 0 {
            return false;
        }
        if self.checkers != 0 {
            if self.checkers.count_ones() > 1 {
                return false;
            }
            let checker = lsb(self.checkers);
            return (bit(to) & (self.checkers | BETWEEN[king][checker])) != 0;
        }
        true
    }

    /// Validate a move from an untrusted source (TT, killers) as pseudo-legal.
    pub fn is_pseudo_legal(&self, mv: Move) -> bool {
        if mv.is_none() {
            return false;
        }
        let us = self.stm;
        let from = mv.from();
        let to = mv.to();
        let flag = mv.flag();
        let code = self.mailbox[from];
        if code == NO_PIECE || code as usize / 6 != us {
            return false;
        }
        let piece = code as usize % 6;
        if self.colors[us] & bit(to) != 0 {
            return false;
        }
        let victim = self.mailbox[to];
        if flag == 6 || flag == 7 {
            return false;
        }
        if flag == EN_PASSANT {
            return piece == PAWN
                && self.ep as usize == to
                && PAWN_ATTACKS[us][from] & bit(to) != 0;
        }
        if mv.is_castle() {
            return piece == KING
                && self.castle_ok(flag)
                && from == if us == WHITE { 4 } else { 60 };
        }
        if mv.is_capture() {
            if victim == NO_PIECE || victim as usize % 6 == KING {
                return false;
            }
        } else if victim != NO_PIECE {
            return false;
        }
        let occ = self.occupied();
        if piece == PAWN {
            let last_rank = if us == WHITE { RANK_8 } else { RANK_1 };
            if (bit(to) & last_rank != 0) != mv.is_promotion() {
                return false;
            }
            if mv.is_capture() {
                return PAWN_ATTACKS[us][from] & bit(to) != 0;
            }
            let forward = |sq: usize| if us == WHITE { sq + 8 } else { sq - 8 };
            if flag == DOUBLE_PUSH {
                let start = if us == WHITE { RANK_2 } else { RANK_7 };
                let mid = forward(from);
                return bit(from) & start != 0
                    && to == forward(mid)
                    && occ & (bit(mid) | bit(to)) == 0;
            }
            return to == forward(from);
        }
        if mv.is_promotion() || flag == DOUBLE_PUSH {
            return false;
        }
        piece_attacks(piece, from, occ) & bit(to) != 0
    }

    /// Castling availability including path and attack checks.
    pub fn castle_ok(&self, flag: u16) -> bool {
        if self.checkers != 0 {
            return false;
        }
        let us = self.stm;
        let them = us ^ 1;
        let occ = self.occupied();
        let (right, empty, safe): (u8, Bitboard, [Square; 2]) = match (us, flag) {
            (WHITE, KING_CASTLE) => (WK_CASTLE, bit(5) | bit(6), [5, 6]),
            (WHITE, _) => (WQ_CASTLE, bit(1) | bit(2) | bit(3), [3, 2]),
            (_, KING_CASTLE) => (BK_CASTLE, bit(61) | bit(62), [61, 62]),
            _ => (BQ_CASTLE, bit(57) | bit(58) | bit(59), [59, 58]),
        };
        self.castling & right != 0
            && occ & empty == 0
            && !self.is_attacked(safe[0], them, occ)
            && !self.is_attacked(safe[1], them, occ)
    }

    pub fn has_non_pawn_material(&self, color: usize) -> bool {
        self.colors[color] & !(self.pieces[PAWN] | self.pieces[KING]) != 0
    }

    /// Draw by insufficient material (KvK, KNvK, KBvK).
    pub fn insufficient_material(&self) -> bool {
        if self.pieces[PAWN] | self.pieces[ROOK] | self.pieces[QUEEN] != 0 {
            return false;
        }
        (self.pieces[KNIGHT] | self.pieces[BISHOP]).count_ones() <= 1
    }

    /// Static exchange evaluation: is the material balance of `mv` >= threshold?
    pub fn see_ge(&self, mv: Move, threshold: i32) -> bool {
        if mv.is_castle() || mv.is_promotion() || mv.flag() == EN_PASSANT {
            return threshold <= 0;
        }
        let from = mv.from();
        let to = mv.to();
        let mut swap = SEE_VALUES[self.piece_on(to).unwrap_or(6)] - threshold;
        if swap < 0 {
            return false;
        }
        swap = SEE_VALUES[self.piece_on(from).unwrap()] - swap;
        if swap <= 0 {
            return true;
        }
        let mut occ = self.occupied() ^ bit(from) ^ bit(to);
        let mut attackers = self.attackers_to(to, occ) & occ;
        let bishops = self.pieces[BISHOP] | self.pieces[QUEEN];
        let rooks = self.pieces[ROOK] | self.pieces[QUEEN];
        let mut stm = self.stm;
        let mut result = 1;
        loop {
            stm ^= 1;
            attackers &= occ;
            let mine = attackers & self.colors[stm];
            if mine == 0 {
                break;
            }
            result ^= 1;
            let mut piece = PAWN;
            while piece <= KING {
                if mine & self.pieces[piece] != 0 {
                    break;
                }
                piece += 1;
            }
            if piece == KING {
                // King can only capture if the opponent has no attackers left.
                if attackers & self.colors[stm ^ 1] != 0 {
                    result ^= 1;
                }
                break;
            }
            swap = SEE_VALUES[piece] - swap;
            if swap < result {
                break;
            }
            let lowest = mine & self.pieces[piece];
            occ ^= 1u64 << lowest.trailing_zeros();
            if piece == PAWN || piece == BISHOP || piece == QUEEN {
                attackers |= bishop_attacks(to, occ) & bishops;
            }
            if piece == ROOK || piece == QUEEN {
                attackers |= rook_attacks(to, occ) & rooks;
            }
        }
        result != 0
    }

    pub fn parse_move(&self, text: &str) -> Option<Move> {
        let mut list = crate::movegen::MoveList::new();
        crate::movegen::generate_all(self, &mut list);
        let found = list
            .iter()
            .find(|m| m.to_uci() == text && self.is_legal(*m));
        found
    }
}

/// Piece values used for SEE and capture ordering (P N B R Q K, empty).
pub const SEE_VALUES: [i32; 7] = [100, 300, 300, 500, 900, 0, 0];
