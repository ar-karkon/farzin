//! Self-play training data generation (`farzin datagen ...`).
//!
//! Each game starts from random opening plies, then both sides play with a
//! soft node limit. Quiet positions are written as 32-byte records with the
//! search score and the final game result (both from White's view).
use crate::board::Board;
use crate::evalnet::EvalNet as Network;
use crate::movegen::legal_moves;
use crate::search::{Limits, Shared, Thread};
use crate::tt::{TranspositionTable, MATE_BOUND};
use crate::types::*;
use std::io::{BufWriter, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub const RECORD_SIZE: usize = 32;

/// Packed position: occupancy, 4-bit piece codes in square order, side to
/// move, white-relative score and result (0 = black win, 1 = draw, 2 = white win).
pub fn pack(board: &Board, score_white: i16, result: u8) -> [u8; RECORD_SIZE] {
    let mut out = [0u8; RECORD_SIZE];
    let occ = board.occupied();
    out[..8].copy_from_slice(&occ.to_le_bytes());
    let mut bb = occ;
    let mut i = 0;
    while bb != 0 {
        let sq = pop_lsb(&mut bb);
        let code = board.mailbox[sq];
        out[8 + i / 2] |= code << (4 * (i % 2));
        i += 1;
    }
    out[24] = board.stm as u8;
    out[25..27].copy_from_slice(&score_white.to_le_bytes());
    out[27] = result;
    out
}

/// Decoded record: (piece code, square) list, side to move, score, result.
pub struct Record {
    pub pieces: Vec<(usize, usize)>,
    pub stm: usize,
    pub score: i16,
    pub result: u8,
}

pub fn unpack(rec: &[u8]) -> Record {
    let occ = u64::from_le_bytes(rec[..8].try_into().unwrap());
    let mut bb = occ;
    let mut pieces = Vec::with_capacity(32);
    let mut i = 0;
    while bb != 0 {
        let sq = pop_lsb(&mut bb);
        let code = (rec[8 + i / 2] >> (4 * (i % 2))) & 15;
        pieces.push((code as usize, sq));
        i += 1;
    }
    Record {
        pieces,
        stm: rec[24] as usize,
        score: i16::from_le_bytes([rec[25], rec[26]]),
        result: rec[27],
    }
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

pub struct Config {
    pub threads: usize,
    pub games: u64,
    pub soft_nodes: u64,
    pub hard_nodes: u64,
    pub random_plies: usize,
    pub out: String,
    pub seed: u64,
    pub net: Option<Arc<Network>>,
    /// Distillation teacher: when set, each stored position is labelled with
    /// this network's static evaluation instead of the search score.
    pub teacher: Option<Arc<Network>>,
}

fn play_game(
    thread: &mut Thread,
    shared: &Shared,
    rng: &mut Rng,
    cfg: &Config,
) -> Vec<[u8; RECORD_SIZE]> {
    // Random opening.
    let mut board = Board::startpos();
    let mut history: Vec<u64> = Vec::new();
    let plies = cfg.random_plies + rng.below(2);
    for _ in 0..plies {
        let moves = legal_moves(&board);
        if moves.is_empty() {
            return Vec::new();
        }
        history.push(board.hash);
        board = board.make_move(moves[rng.below(moves.len())]);
    }
    if legal_moves(&board).is_empty() {
        return Vec::new();
    }
    thread.histories.clear();
    // SAFETY-free reset: the shared table belongs to this worker only.
    let limits = Limits {
        nodes: Some(cfg.hard_nodes),
        soft_nodes: Some(cfg.soft_nodes),
        ..Default::default()
    };
    let mut positions: Vec<(Board, i16)> = Vec::new();
    let mut win_plies = 0;
    let mut draw_plies = 0;
    let result: u8;
    let mut first = true;
    loop {
        if legal_moves(&board).is_empty() {
            result = if board.in_check() {
                if board.stm == WHITE {
                    0
                } else {
                    2
                }
            } else {
                1
            };
            break;
        }
        if board.halfmove >= 100 || board.insufficient_material() || is_threefold(&board, &history)
        {
            result = 1;
            break;
        }
        shared.stop.store(false, Ordering::Relaxed);
        shared.nodes.store(0, Ordering::Relaxed);
        shared.tt.new_search();
        let r = thread.iterate(shared, &board, &history, limits, Instant::now());
        let score = r.score;
        if first && score.abs() > 400 {
            return Vec::new(); // unbalanced opening
        }
        first = false;
        let white_score = if board.stm == WHITE { score } else { -score };
        if score.abs() >= 2000 {
            win_plies += 1;
        } else {
            win_plies = 0;
        }
        if win_plies >= 4 {
            result = if white_score > 0 { 2 } else { 0 };
            break;
        }
        if score.abs() <= 10 && positions.len() > 70 {
            draw_plies += 1;
        } else {
            draw_plies = 0;
        }
        if draw_plies >= 12 {
            result = 1;
            break;
        }
        if history.len() > 500 {
            result = 1;
            break;
        }
        if !board.in_check() && !r.best.is_noisy() && score.abs() < MATE_BOUND {
            let label = match &cfg.teacher {
                Some(t) => {
                    let v = t.evaluate(&board);
                    if board.stm == WHITE {
                        v
                    } else {
                        -v
                    }
                }
                None => white_score,
            };
            positions.push((board, label.clamp(-20000, 20000) as i16));
        }
        history.push(board.hash);
        board = board.make_move(r.best);
    }
    positions.iter().map(|(b, s)| pack(b, *s, result)).collect()
}

fn is_threefold(board: &Board, history: &[u64]) -> bool {
    let n = history.len();
    let limit = (board.halfmove as usize).min(n);
    let mut count = 0;
    let mut i = 2;
    while i <= limit {
        if history[n - i] == board.hash {
            count += 1;
            if count >= 2 {
                return true;
            }
        }
        i += 2;
    }
    false
}

pub fn run(cfg: Config) {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&cfg.out)
        .expect("open output");
    let writer = Arc::new(Mutex::new(BufWriter::new(file)));
    let games = Arc::new(AtomicU64::new(0));
    let positions = Arc::new(AtomicU64::new(0));
    let done = Arc::new(AtomicBool::new(false));
    let start = Instant::now();
    let cfg = Arc::new(cfg);
    let mut handles = Vec::new();
    for t in 0..cfg.threads {
        let (writer, games, positions, cfg, done) = (
            writer.clone(),
            games.clone(),
            positions.clone(),
            cfg.clone(),
            done.clone(),
        );
        handles.push(std::thread::spawn(move || {
            let shared = Shared {
                tt: TranspositionTable::new(16),
                stop: AtomicBool::new(false),
                nodes: AtomicU64::new(0),
            };
            let mut thread = Thread::new(0);
            thread.silent = true;
            thread.net = cfg.net.clone();
            let mut rng = Rng(cfg.seed ^ (0x9E37_79B9_7F4A_7C15u64.wrapping_mul(t as u64 + 1)) | 1);
            let mut shared = shared;
            while !done.load(Ordering::Relaxed) {
                shared.tt.clear();
                let recs = play_game(&mut thread, &shared, &mut rng, &cfg);
                if recs.is_empty() {
                    continue;
                }
                {
                    let mut w = writer.lock().unwrap();
                    for r in &recs {
                        w.write_all(r).unwrap();
                    }
                }
                positions.fetch_add(recs.len() as u64, Ordering::Relaxed);
                let g = games.fetch_add(1, Ordering::Relaxed) + 1;
                if g % 200 == 0 {
                    let p = positions.load(Ordering::Relaxed);
                    let secs = start.elapsed().as_secs_f64();
                    eprintln!("games {g} positions {p} ({:.0} pos/s)", p as f64 / secs);
                    writer.lock().unwrap().flush().unwrap();
                }
                if g >= cfg.games {
                    done.store(true, Ordering::Relaxed);
                }
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    writer.lock().unwrap().flush().unwrap();
    eprintln!(
        "done: games {} positions {}",
        games.load(Ordering::Relaxed),
        positions.load(Ordering::Relaxed)
    );
}
