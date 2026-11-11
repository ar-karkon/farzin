//! Principal variation search with Lazy SMP.
use crate::board::Board;
use crate::evalnet::{EvalAccs, EvalNet};
use crate::movegen::{legal_moves, MoveList};
use crate::movepick::{MovePicker, OrderingContext};
use crate::tt::*;
use crate::types::*;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

pub const MAX_PLY: usize = 128;
const HISTORY_MAX: i32 = 16_384;
const CORR_SIZE: usize = 16_384;
const CORR_GRAIN: i32 = 256;
const CORR_MAX: i32 = CORR_GRAIN * 64;
const PAWN_HIST: usize = 512;

/// Tunable search parameters (centipawns / plies), kept together so
/// experiments can override them through UCI options.
#[derive(Clone, Copy, Debug)]
pub struct Params {
    pub rfp_margin: i32,
    pub rfp_depth: i32,
    pub nmp_base: i32,
    pub nmp_div: i32,
    pub razor_margin: i32,
    pub futility_base: i32,
    pub futility_mult: i32,
    pub see_quiet: i32,
    pub see_noisy: i32,
    pub lmr_quiet_base: f64,
    pub lmr_quiet_div: f64,
    pub lmr_noisy_base: f64,
    pub lmr_noisy_div: f64,
    pub hist_div: i32,
    pub probcut_margin: i32,
    pub singular_depth: i32,
    pub asp_delta: i32,
    pub qs_futility: i32,
    pub hist_prune: i32,
    /// Experiment switches (0 = previous behaviour); see `negamax`.
    pub x_nmp_all: i32,
    pub x_deeper: i32,
    pub x_tm: i32,
    pub x_cont_corr: i32,
    pub x_cutoff: i32,
    pub x_checkext: i32,
    pub x_optimism: i32,
    /// Soft time target scale in percent.
    pub tm_pct: i32,
    /// Farzin 0.5 search features (1 = on); kept switchable for ablation.
    pub x_prior: i32,
    pub x_pawnhist: i32,
    pub x_evalhist: i32,
    pub x_malus: i32,
    pub x_frac: i32,
    pub x_postlmr: i32,
    pub x_minorcorr: i32,
    pub x_hindsight: i32,
    pub x_smallpc: i32,
    pub x_threats: i32,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            rfp_margin: 75,
            rfp_depth: 8,
            nmp_base: 4,
            nmp_div: 200,
            razor_margin: 250,
            futility_base: 90,
            futility_mult: 110,
            see_quiet: 30,
            see_noisy: 95,
            lmr_quiet_base: 0.80,
            lmr_quiet_div: 2.25,
            lmr_noisy_base: 0.20,
            lmr_noisy_div: 3.30,
            hist_div: 8192,
            probcut_margin: 200,
            singular_depth: 7,
            asp_delta: 14,
            qs_futility: 140,
            hist_prune: 3500,
            x_nmp_all: 0,
            x_deeper: 0,
            x_tm: 0,
            x_cont_corr: 0,
            x_cutoff: 0,
            x_checkext: 0,
            x_optimism: 0,
            tm_pct: 100,
            x_prior: 1,
            x_pawnhist: 1,
            x_evalhist: 1,
            x_malus: 1,
            x_frac: 1,
            x_postlmr: 1,
            x_minorcorr: 1,
            x_hindsight: 1,
            x_smallpc: 1,
            x_threats: 1,
        }
    }
}

pub struct Histories {
    pub main: Box<[[[i16; 64]; 64]; 2]>,
    /// `cont[prev_piece * 64 + prev_to][piece * 64 + to]`.
    pub cont: Box<[[i16; 768]; 768]>,
    /// `capture[piece_code][to][victim_type]`.
    pub capture: Box<[[[i16; 6]; 64]; 12]>,
    pub counter: Box<[[Move; 64]; 12]>,
    pub killers: [[Move; 2]; MAX_PLY + 4],
    pub pawn_corr: Box<[[i32; CORR_SIZE]; 2]>,
    pub non_pawn_corr: Box<[[[i32; CORR_SIZE]; 2]; 2]>,
    /// Correction keyed by the previous two moves (`piece*64+to` pairs).
    pub cont_corr: Box<[[i32; 768]; 768]>,
    /// `pawn[pawn_hash % PAWN_HIST][piece_code][to]`.
    pub pawn: Box<[[[i16; 64]; 12]; PAWN_HIST]>,
    pub minor_corr: Box<[[i32; CORR_SIZE]; 2]>,
}

fn zeroed_box<T: Copy>(value: T) -> Box<T> {
    Box::new(value)
}

impl Histories {
    pub fn new() -> Self {
        Histories {
            main: zeroed_box([[[0; 64]; 64]; 2]),
            cont: vec![[0i16; 768]; 768]
                .into_boxed_slice()
                .try_into()
                .unwrap(),
            capture: zeroed_box([[[0; 6]; 64]; 12]),
            counter: zeroed_box([[Move::NONE; 64]; 12]),
            killers: [[Move::NONE; 2]; MAX_PLY + 4],
            pawn_corr: vec![[0i32; CORR_SIZE]; 2]
                .into_boxed_slice()
                .try_into()
                .unwrap(),
            non_pawn_corr: vec![[[0i32; CORR_SIZE]; 2]; 2]
                .into_boxed_slice()
                .try_into()
                .unwrap(),
            cont_corr: vec![[0i32; 768]; 768]
                .into_boxed_slice()
                .try_into()
                .unwrap(),
            pawn: vec![[[0i16; 64]; 12]; PAWN_HIST]
                .into_boxed_slice()
                .try_into()
                .unwrap(),
            minor_corr: vec![[0i32; CORR_SIZE]; 2]
                .into_boxed_slice()
                .try_into()
                .unwrap(),
        }
    }

    pub fn clear(&mut self) {
        *self = Histories::new();
    }

    #[inline]
    pub fn quiet_score(&self, board: &Board, stm: usize, mv: Move, ctx: &OrderingContext) -> i32 {
        let piece = board.mailbox[mv.from()] as usize;
        let idx = piece * 64 + mv.to();
        let mut score = i32::from(self.main[stm][mv.from()][mv.to()]);
        if let Some(c) = ctx.cont[0] {
            score += i32::from(self.cont[c][idx]);
        }
        if let Some(c) = ctx.cont[1] {
            score += i32::from(self.cont[c][idx]);
        }
        if let Some(c) = ctx.cont[2] {
            score += i32::from(self.cont[c][idx]) / 2;
        }
        if let Some(pi) = ctx.pawn {
            score += i32::from(self.pawn[pi][piece][mv.to()]);
        }
        score
    }
}

impl Default for Histories {
    fn default() -> Self {
        Self::new()
    }
}

#[inline]
fn gravity(entry: &mut i16, bonus: i32) {
    let bonus = bonus.clamp(-HISTORY_MAX, HISTORY_MAX);
    let v = i32::from(*entry);
    *entry = (v + bonus - v * bonus.abs() / HISTORY_MAX) as i16;
}

fn history_bonus(depth: i32) -> i32 {
    (170 * depth - 100).clamp(0, 2000)
}

fn history_malus(depth: i32, on: bool) -> i32 {
    if on {
        (260 * depth - 120).clamp(0, 2400)
    } else {
        history_bonus(depth)
    }
}

#[inline]
fn pawn_index(board: &Board) -> usize {
    (board.pawn_hash as usize) % PAWN_HIST
}

#[derive(Clone, Copy)]
struct StackEntry {
    static_eval: i32,
    mv: Move,
    /// `piece_code * 64 + to` of the move played from this ply.
    cont: Option<usize>,
    excluded: Move,
    tt_pv: bool,
    double_ext: i32,
    cutoff_cnt: i32,
    /// LMR reduction applied to the move searched from this ply.
    reduction: i32,
}

impl Default for StackEntry {
    fn default() -> Self {
        StackEntry {
            static_eval: NO_SCORE,
            mv: Move::NONE,
            cont: None,
            excluded: Move::NONE,
            tt_pv: false,
            double_ext: 0,
            cutoff_cnt: 0,
            reduction: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Limits {
    pub depth: Option<i32>,
    pub nodes: Option<u64>,
    /// Soft node limit (stop between iterations); used for data generation.
    pub soft_nodes: Option<u64>,
    pub soft_ms: Option<u64>,
    pub hard_ms: Option<u64>,
    pub infinite: bool,
}

pub struct Shared {
    pub tt: TranspositionTable,
    pub stop: AtomicBool,
    pub nodes: AtomicU64,
}

#[derive(Clone, Debug, Default)]
pub struct SearchResult {
    pub best: Move,
    pub ponder: Move,
    pub score: i32,
    pub depth: i32,
    pub nodes: u64,
    pub pv: Vec<Move>,
}

pub struct Thread {
    pub id: usize,
    pub histories: Histories,
    stack: Vec<StackEntry>,
    keys: Vec<u64>,
    pv: Vec<[Move; MAX_PLY + 1]>,
    pv_len: [usize; MAX_PLY + 1],
    pub nodes: u64,
    flushed_nodes: u64,
    seldepth: usize,
    stopped: bool,
    start: Instant,
    limits: Limits,
    root_depth: i32,
    root_best: Move,
    root_nodes: Box<[[u64; 64]; 64]>,
    pub params: Params,
    lmr: Box<[[[i32; 64]; 64]; 2]>,
    /// Same reductions in 1/1024 plies.
    lmr_f: Box<[[[i32; 64]; 64]; 2]>,
    pub net: Option<Arc<EvalNet>>,
    acc: EvalAccs,
    pub silent: bool,
    /// Distillation sampler: records a fraction of the positions evaluated
    /// inside the search tree (see `datagen tree_rate=`).
    pub sampler: Option<Box<Sampler>>,
    /// Root-side optimism (centipawns), indexed by side to move.
    optimism: [i32; 2],
}

/// Records roughly one in `rate` static evaluations made by the search,
/// labelled with the (undamped) network value, white-relative.
pub struct Sampler {
    pub rate: u64,
    /// Skip positions where the side to move has a winning capture
    /// (SEE > 0) or is in check, i.e. keep "quiet" positions only.
    pub quiet_only: bool,
    pub rng: u64,
    pub out: Vec<[u8; crate::datagen::RECORD_SIZE]>,
}

/// Not in check and no capture/promotion with positive SEE.
pub fn is_quiet(board: &Board) -> bool {
    if board.in_check() {
        return false;
    }
    let mut list = MoveList::new();
    crate::movegen::generate_noisy(board, &mut list);
    let noisy = list.iter().any(|m| board.is_legal(m) && board.see_ge(m, 1));
    !noisy
}

impl Sampler {
    #[inline]
    fn hit(&mut self) -> bool {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng.is_multiple_of(self.rate)
    }
}

pub fn format_score(score: i32) -> String {
    if score.abs() >= MATE_BOUND {
        let plies = MATE - score.abs();
        let moves = (plies + 1) / 2;
        format!("mate {}", if score > 0 { moves } else { -moves })
    } else {
        format!("cp {score}")
    }
}

type LmrTable = Box<[[[i32; 64]; 64]; 2]>;

fn init_lmr(p: &Params) -> (LmrTable, LmrTable) {
    let mut t = Box::new([[[0; 64]; 64]; 2]);
    let mut f = Box::new([[[0; 64]; 64]; 2]);
    for d in 1..64 {
        for m in 1..64 {
            let l = (d as f64).ln() * (m as f64).ln();
            let q = p.lmr_quiet_base + l / p.lmr_quiet_div;
            let n = p.lmr_noisy_base + l / p.lmr_noisy_div;
            t[0][d][m] = q as i32;
            t[1][d][m] = n as i32;
            f[0][d][m] = (q * 1024.0) as i32;
            f[1][d][m] = (n * 1024.0) as i32;
        }
    }
    (t, f)
}

static NULL_NET: OnceLock<()> = OnceLock::new();

impl Thread {
    pub fn new(id: usize) -> Self {
        let _ = NULL_NET.get_or_init(|| ());
        let params = Params::default();
        let (lmr, lmr_f) = init_lmr(&params);
        Thread {
            id,
            histories: Histories::new(),
            stack: vec![StackEntry::default(); MAX_PLY + 8],
            keys: Vec::with_capacity(1024),
            pv: vec![[Move::NONE; MAX_PLY + 1]; MAX_PLY + 1],
            pv_len: [0; MAX_PLY + 1],
            nodes: 0,
            flushed_nodes: 0,
            seldepth: 0,
            stopped: false,
            start: Instant::now(),
            limits: Limits::default(),
            root_depth: 0,
            root_best: Move::NONE,
            root_nodes: Box::new([[0; 64]; 64]),
            lmr,
            lmr_f,
            params,
            net: None,
            acc: EvalAccs::default(),
            silent: false,
            sampler: None,
            optimism: [0; 2],
        }
    }

    pub fn set_params(&mut self, params: Params) {
        self.params = params;
        (self.lmr, self.lmr_f) = init_lmr(&params);
    }

    #[inline]
    fn evaluate(&mut self, board: &Board, ply: usize) -> i32 {
        match &self.net {
            Some(net) => {
                let v = self.acc.evaluate(net, board, ply);
                if let Some(s) = &mut self.sampler {
                    if s.hit() && (!s.quiet_only || is_quiet(board)) {
                        let w = if board.stm == WHITE { v } else { -v };
                        s.out.push(crate::datagen::pack(
                            board,
                            w.clamp(-20000, 20000) as i16,
                            1,
                        ));
                    }
                }
                let v = v + self.optimism[board.stm];
                // Damp toward zero as the 50-move counter grows.
                v * (200 - i32::from(board.halfmove)) / 200
            }
            None => crate::eval::evaluate(board),
        }
    }

    #[inline]
    fn cont_corr_key(&self, ply: usize) -> Option<(usize, usize)> {
        if self.params.x_cont_corr == 0 || ply < 2 {
            return None;
        }
        match (self.stack[ply - 1].cont, self.stack[ply - 2].cont) {
            (Some(a), Some(b)) => Some((b, a)),
            _ => None,
        }
    }

    #[inline]
    fn corrected(&self, board: &Board, raw: i32, ply: usize) -> i32 {
        let h = &self.histories;
        let stm = board.stm;
        let p = h.pawn_corr[stm][(board.pawn_hash as usize) % CORR_SIZE];
        let w = h.non_pawn_corr[stm][WHITE][(board.non_pawn_hash[WHITE] as usize) % CORR_SIZE];
        let b = h.non_pawn_corr[stm][BLACK][(board.non_pawn_hash[BLACK] as usize) % CORR_SIZE];
        let c = self
            .cont_corr_key(ply)
            .map_or(0, |(a, b)| h.cont_corr[a][b]);
        let m = if self.params.x_minorcorr != 0 {
            h.minor_corr[stm][(board.minor_hash as usize) % CORR_SIZE]
        } else {
            0
        };
        let corr = (2 * p + w + b + c + m) / (2 * CORR_GRAIN);
        (raw + corr).clamp(-MATE_BOUND + 1, MATE_BOUND - 1)
    }

    fn update_correction(&mut self, board: &Board, depth: i32, diff: i32, ply: usize) {
        let stm = board.stm;
        let scaled = (diff * CORR_GRAIN).clamp(-CORR_MAX, CORR_MAX);
        let w = (depth + 1).min(16);
        let upd = |e: &mut i32| {
            *e = ((*e * (256 - w) + scaled * w) / 256).clamp(-CORR_MAX, CORR_MAX);
        };
        let h = &mut self.histories;
        upd(&mut h.pawn_corr[stm][(board.pawn_hash as usize) % CORR_SIZE]);
        upd(&mut h.non_pawn_corr[stm][WHITE][(board.non_pawn_hash[WHITE] as usize) % CORR_SIZE]);
        upd(&mut h.non_pawn_corr[stm][BLACK][(board.non_pawn_hash[BLACK] as usize) % CORR_SIZE]);
        if self.params.x_minorcorr != 0 {
            upd(&mut h.minor_corr[stm][(board.minor_hash as usize) % CORR_SIZE]);
        }
        if let Some((a, b)) = self.cont_corr_key(ply) {
            upd(&mut self.histories.cont_corr[a][b]);
        }
    }

    fn ctx(&self, board: &Board, ply: usize) -> OrderingContext {
        let get = |back: usize| {
            if ply >= back {
                self.stack[ply - back].cont
            } else {
                None
            }
        };
        OrderingContext {
            cont: [get(1), get(2), get(4)],
            pawn: (self.params.x_pawnhist != 0).then(|| pawn_index(board)),
            threats: self.params.x_threats != 0,
        }
    }

    #[inline]
    fn check_stop(&mut self, shared: &Shared) -> bool {
        if self.stopped {
            return true;
        }
        if self.nodes & 1023 == 0 {
            shared
                .nodes
                .fetch_add(self.nodes - self.flushed_nodes, Ordering::Relaxed);
            self.flushed_nodes = self.nodes;
            if shared.stop.load(Ordering::Relaxed) {
                self.stopped = true;
            } else if self.id == 0 {
                if let Some(hard) = self.limits.hard_ms {
                    if self.start.elapsed().as_millis() as u64 >= hard {
                        self.stopped = true;
                    }
                }
            }
        }
        if let Some(limit) = self.limits.nodes {
            // Exact for one thread; helpers count toward the shared total.
            let total =
                if self.id == 0 && shared.nodes.load(Ordering::Relaxed) == self.flushed_nodes {
                    self.nodes
                } else {
                    shared.nodes.load(Ordering::Relaxed) + (self.nodes - self.flushed_nodes)
                };
            if total >= limit {
                self.stopped = true;
            }
        }
        if self.stopped && self.id == 0 {
            shared.stop.store(true, Ordering::Relaxed);
        }
        self.stopped
    }

    fn is_repetition(&self, board: &Board) -> bool {
        let n = self.keys.len();
        let limit = (board.halfmove as usize).min(n);
        let mut i = 4;
        while i <= limit {
            if self.keys[n - i] == board.hash {
                return true;
            }
            i += 2;
        }
        false
    }

    #[inline]
    fn draw_score(&self) -> i32 {
        1 - (self.nodes & 2) as i32
    }

    fn update_pv(&mut self, ply: usize, mv: Move) {
        self.pv[ply][ply] = mv;
        let child_len = self.pv_len[ply + 1];
        for i in ply + 1..child_len {
            self.pv[ply][i] = self.pv[ply + 1][i];
        }
        self.pv_len[ply] = child_len.max(ply + 1);
    }

    #[inline]
    fn push_move(&mut self, board: &Board, child: &Board, mv: Move, ply: usize) {
        self.stack[ply].mv = mv;
        self.stack[ply].cont = Some(board.mailbox[mv.from()] as usize * 64 + mv.to());
        self.keys.push(board.hash);
        if let Some(net) = &self.net {
            self.acc.push(net, board, child, mv, ply + 1);
        }
    }

    #[inline]
    fn pop_move(&mut self) {
        self.keys.pop();
    }

    fn qsearch(
        &mut self,
        shared: &Shared,
        board: &Board,
        mut alpha: i32,
        beta: i32,
        ply: usize,
    ) -> i32 {
        self.nodes += 1;
        if self.check_stop(shared) {
            return 0;
        }
        let pv_node = beta - alpha > 1;
        self.pv_len[ply] = ply;
        self.seldepth = self.seldepth.max(ply);
        if board.halfmove >= 100 || board.insufficient_material() || self.is_repetition(board) {
            return self.draw_score();
        }
        let in_check = board.in_check();
        if ply >= MAX_PLY {
            return if in_check {
                0
            } else {
                self.evaluate(board, ply)
            };
        }
        let tt = shared.tt.probe(board.hash);
        let mut tt_move = Move::NONE;
        let mut tt_score = NO_SCORE;
        let mut tt_pv = pv_node;
        if let Some(t) = tt {
            tt_move = t.mv;
            tt_score = score_from_tt(t.score, ply, board.halfmove);
            tt_pv |= t.pv;
            if !pv_node
                && (t.bound == BOUND_EXACT
                    || (t.bound == BOUND_LOWER && tt_score >= beta)
                    || (t.bound == BOUND_UPPER && tt_score <= alpha))
            {
                return tt_score;
            }
        }
        let mut best = -INF;
        let mut raw_eval = NO_SCORE;
        let mut futility = -INF;
        if !in_check {
            raw_eval = match tt {
                Some(t) if t.eval != NO_SCORE => t.eval,
                _ => self.evaluate(board, ply),
            };
            let mut eval = self.corrected(board, raw_eval, ply);
            if let Some(t) = tt {
                if tt_score != NO_SCORE
                    && (t.bound == BOUND_EXACT
                        || (t.bound == BOUND_LOWER && tt_score > eval)
                        || (t.bound == BOUND_UPPER && tt_score < eval))
                {
                    eval = tt_score;
                }
            }
            if eval >= beta {
                if tt.is_none() {
                    shared.tt.store(
                        board.hash,
                        Move::NONE,
                        score_to_tt(eval, ply),
                        raw_eval,
                        0,
                        BOUND_LOWER,
                        false,
                    );
                }
                return if eval.abs() < MATE_BOUND {
                    (eval + beta) / 2
                } else {
                    eval
                };
            }
            alpha = alpha.max(eval);
            best = eval;
            futility = eval + self.params.qs_futility;
        }
        let original_alpha = alpha;
        let mut best_move = Move::NONE;
        let mut picker = MovePicker::new_qsearch(board, tt_move, self.ctx(board, ply));
        let mut moves = 0;
        while let Some(mv) = picker.next(board, &self.histories) {
            if !board.is_legal(mv) {
                continue;
            }
            moves += 1;
            if best > -MATE_BOUND {
                if !in_check {
                    if !mv.is_promotion() {
                        let victim = if mv.flag() == EN_PASSANT {
                            100
                        } else {
                            crate::board::SEE_VALUES[board.piece_on(mv.to()).unwrap_or(6)]
                        };
                        if futility + victim <= alpha && !board.see_ge(mv, 1) {
                            best = best.max(futility + victim);
                            continue;
                        }
                    }
                    if !board.see_ge(mv, -20) {
                        continue;
                    }
                } else if moves > 2 && !mv.is_noisy() {
                    // In check: once a safe evasion exists, skip late quiets.
                    continue;
                }
            }
            let child = board.make_move(mv);
            shared.tt.prefetch(child.hash);
            self.push_move(board, &child, mv, ply);
            let score = -self.qsearch(shared, &child, -beta, -alpha, ply + 1);
            self.pop_move();
            if self.stopped {
                return 0;
            }
            if score > best {
                best = score;
                if score > alpha {
                    best_move = mv;
                    alpha = score;
                    if pv_node {
                        self.update_pv(ply, mv);
                    }
                    if score >= beta {
                        break;
                    }
                }
            }
        }
        if in_check && moves == 0 {
            return -MATE + ply as i32;
        }
        let bound = if best >= beta {
            BOUND_LOWER
        } else if best > original_alpha && pv_node {
            BOUND_EXACT
        } else {
            BOUND_UPPER
        };
        shared.tt.store(
            board.hash,
            best_move,
            score_to_tt(best, ply),
            raw_eval,
            0,
            bound,
            tt_pv,
        );
        best
    }

    #[allow(clippy::too_many_arguments)]
    fn negamax(
        &mut self,
        shared: &Shared,
        board: &Board,
        mut alpha: i32,
        mut beta: i32,
        mut depth: i32,
        ply: usize,
        cut_node: bool,
    ) -> i32 {
        let pv_node = beta - alpha > 1;
        let root = ply == 0;
        if depth <= 0 {
            return self.qsearch(shared, board, alpha, beta, ply);
        }
        self.nodes += 1;
        if self.check_stop(shared) {
            return 0;
        }
        self.pv_len[ply] = ply;
        self.seldepth = self.seldepth.max(ply);
        let in_check = board.in_check();
        if !root {
            if board.halfmove >= 100 {
                // Checkmate takes precedence over the 50-move rule.
                if in_check && legal_moves(board).is_empty() {
                    return -MATE + ply as i32;
                }
                return self.draw_score();
            }
            if board.insufficient_material() || self.is_repetition(board) {
                return self.draw_score();
            }
            if ply >= MAX_PLY {
                return if in_check {
                    0
                } else {
                    self.evaluate(board, ply)
                };
            }
            alpha = alpha.max(-MATE + ply as i32);
            beta = beta.min(MATE - ply as i32 - 1);
            if alpha >= beta {
                return alpha;
            }
        }
        let excluded = self.stack[ply].excluded;
        let tt = if excluded.is_none() {
            shared.tt.probe(board.hash)
        } else {
            None
        };
        let mut tt_move = if root { self.root_best } else { Move::NONE };
        let mut tt_score = NO_SCORE;
        let mut tt_depth = -1;
        let mut tt_bound = BOUND_NONE;
        if let Some(t) = tt {
            if !root {
                tt_move = t.mv;
            }
            tt_score = score_from_tt(t.score, ply, board.halfmove);
            tt_depth = t.depth;
            tt_bound = t.bound;
        }
        let tt_pv = pv_node || tt.is_some_and(|t| t.pv);
        self.stack[ply].tt_pv = tt_pv;
        if !pv_node
            && excluded.is_none()
            && tt_depth >= depth
            && board.halfmove < 90
            && (tt_bound == BOUND_EXACT
                || (tt_bound == BOUND_LOWER && tt_score >= beta)
                || (tt_bound == BOUND_UPPER && tt_score <= alpha))
        {
            if tt_score >= beta
                && !tt_move.is_none()
                && !tt_move.is_noisy()
                && board.is_pseudo_legal(tt_move)
            {
                self.update_quiet_stats(board, ply, tt_move, &[], depth);
            }
            return tt_score;
        }

        // Static evaluation.
        let raw_eval;
        let mut eval;
        if in_check {
            raw_eval = NO_SCORE;
            eval = NO_SCORE;
            self.stack[ply].static_eval = NO_SCORE;
        } else if !excluded.is_none() {
            raw_eval = NO_SCORE;
            eval = self.stack[ply].static_eval;
        } else {
            raw_eval = match tt {
                Some(t) if t.eval != NO_SCORE => t.eval,
                _ => self.evaluate(board, ply),
            };
            let corrected = self.corrected(board, raw_eval, ply);
            self.stack[ply].static_eval = corrected;
            eval = corrected;
            if tt_score != NO_SCORE
                && (tt_bound == BOUND_EXACT
                    || (tt_bound == BOUND_LOWER && tt_score > eval)
                    || (tt_bound == BOUND_UPPER && tt_score < eval))
            {
                eval = tt_score;
            }
            if tt.is_none() {
                shared.tt.store(
                    board.hash,
                    Move::NONE,
                    NO_SCORE,
                    raw_eval,
                    0,
                    BOUND_NONE,
                    tt_pv,
                );
            }
        }
        let static_eval = self.stack[ply].static_eval;
        let improving = !in_check
            && ply >= 2
            && if self.stack[ply - 2].static_eval != NO_SCORE {
                static_eval > self.stack[ply - 2].static_eval
            } else {
                ply >= 4
                    && self.stack[ply - 4].static_eval != NO_SCORE
                    && static_eval > self.stack[ply - 4].static_eval
            };
        let p = self.params;
        let prior_reduction = if ply >= 1 {
            self.stack[ply - 1].reduction
        } else {
            0
        };
        self.stack[ply].reduction = 0;
        if !in_check && excluded.is_none() && ply >= 1 {
            let prev_eval = self.stack[ply - 1].static_eval;
            let prev_mv = self.stack[ply - 1].mv;
            if prev_eval != NO_SCORE {
                // Eval-difference history: reward the opponent's previous quiet
                // move if it made their position better (our eval fell).
                if p.x_evalhist != 0 && !prev_mv.is_none() && !prev_mv.is_noisy() {
                    let bonus = (-10 * (prev_eval + static_eval)).clamp(-1600, 1400) + 600;
                    gravity(
                        &mut self.histories.main[board.stm ^ 1][prev_mv.from()][prev_mv.to()],
                        bonus,
                    );
                }
                // Hindsight depth adjustment based on the parent's reduction.
                if p.x_hindsight != 0 && !pv_node {
                    let opponent_worsening = static_eval > -prev_eval;
                    if prior_reduction >= 3 && !opponent_worsening {
                        depth += 1;
                    } else if prior_reduction >= 1 && depth >= 2 && static_eval + prev_eval > 175 {
                        depth -= 1;
                    }
                }
            }
        }
        self.histories.killers[ply + 1] = [Move::NONE; 2];
        self.stack[ply + 1].excluded = Move::NONE;
        self.stack[ply + 2].double_ext = self.stack[ply].double_ext;
        self.stack[ply + 2].cutoff_cnt = 0;

        if !pv_node && !in_check && excluded.is_none() {
            // Reverse futility pruning.
            if depth <= p.rfp_depth
                && eval < MATE_BOUND
                && eval - p.rfp_margin * (depth - i32::from(improving)) >= beta
            {
                return (eval + beta) / 2;
            }
            // Razoring.
            if depth <= 3 && eval + p.razor_margin * depth < alpha {
                let v = self.qsearch(shared, board, alpha, alpha + 1, ply);
                if v <= alpha {
                    return v;
                }
            }
            // Null move pruning.
            if (cut_node || p.x_nmp_all != 0)
                && depth >= 3
                && eval >= beta
                && static_eval >= beta - 15 * depth + 150
                && ply >= 1
                && !self.stack[ply - 1].mv.is_none()
                && board.has_non_pawn_material(board.stm)
                && beta > -MATE_BOUND
            {
                let r = p.nmp_base + depth / 3 + ((eval - beta) / p.nmp_div).min(3);
                let null = board.make_null();
                self.stack[ply].mv = Move::NONE;
                self.stack[ply].cont = None;
                self.keys.push(board.hash);
                if let Some(net) = &self.net {
                    self.acc.push_null(net, ply + 1);
                }
                let v = -self.negamax(shared, &null, -beta, -beta + 1, depth - r, ply + 1, false);
                self.keys.pop();
                if self.stopped {
                    return 0;
                }
                if v >= beta {
                    let v = if v >= MATE_BOUND { beta } else { v };
                    if depth < 14 {
                        return v;
                    }
                    let verify = self.negamax(shared, board, beta - 1, beta, depth - r, ply, false);
                    if verify >= beta {
                        return v;
                    }
                }
            }
            // ProbCut.
            let pc_beta = beta + p.probcut_margin - 50 * i32::from(improving);
            if depth >= 5
                && beta.abs() < MATE_BOUND
                && !(tt_depth >= depth - 3 && tt_score != NO_SCORE && tt_score < pc_beta)
            {
                let mut picker = MovePicker::new_qsearch(board, tt_move, self.ctx(board, ply));
                while let Some(mv) = picker.next(board, &self.histories) {
                    if !mv.is_noisy()
                        || !board.see_ge(mv, pc_beta - static_eval)
                        || !board.is_legal(mv)
                    {
                        continue;
                    }
                    let child = board.make_move(mv);
                    self.push_move(board, &child, mv, ply);
                    let mut v = -self.qsearch(shared, &child, -pc_beta, -pc_beta + 1, ply + 1);
                    if v >= pc_beta {
                        v = -self.negamax(
                            shared,
                            &child,
                            -pc_beta,
                            -pc_beta + 1,
                            depth - 4,
                            ply + 1,
                            !cut_node,
                        );
                    }
                    self.pop_move();
                    if self.stopped {
                        return 0;
                    }
                    if v >= pc_beta {
                        shared.tt.store(
                            board.hash,
                            mv,
                            score_to_tt(v, ply),
                            raw_eval,
                            depth - 3,
                            BOUND_LOWER,
                            tt_pv,
                        );
                        return v;
                    }
                }
            }
        }

        // Small ProbCut: a deep enough TT lower bound far above beta.
        if p.x_smallpc != 0
            && !pv_node
            && excluded.is_none()
            && tt_bound & BOUND_LOWER != 0
            && tt_depth >= depth - 4
            && tt_score != NO_SCORE
            && tt_score >= beta + 400
            && tt_score.abs() < MATE_BOUND
            && beta.abs() < MATE_BOUND
        {
            return beta + 400;
        }

        // Internal iterative reduction.
        if depth >= 4 && tt_move.is_none() && (pv_node || cut_node) {
            depth -= 1;
        }

        let counter = if ply >= 1 {
            match self.stack[ply - 1].cont {
                Some(c) => self.histories.counter[c / 64][c % 64],
                None => Move::NONE,
            }
        } else {
            Move::NONE
        };
        let ctx = self.ctx(board, ply);
        let mut picker = MovePicker::new(board, tt_move, self.histories.killers[ply], counter, ctx);
        let original_alpha = alpha;
        let mut best_score = -INF;
        let mut best_move = Move::NONE;
        let mut move_count = 0;
        let mut quiets: [Move; 64] = [Move::NONE; 64];
        let mut quiet_count = 0;
        let mut noisies: [Move; 32] = [Move::NONE; 32];
        let mut noisy_count = 0;
        let lmp_limit = (3 + depth * depth) / (2 - i32::from(improving));
        let tt_noisy = !tt_move.is_none() && tt_move.is_noisy();

        while let Some(mv) = picker.next(board, &self.histories) {
            if mv == excluded || !board.is_legal(mv) {
                continue;
            }
            move_count += 1;
            let quiet = !mv.is_noisy();
            let hist = if quiet {
                self.histories.quiet_score(board, board.stm, mv, &ctx)
            } else {
                let piece = board.mailbox[mv.from()] as usize;
                let victim = board.piece_on(mv.to()).unwrap_or(PAWN);
                i32::from(self.histories.capture[piece][mv.to()][victim])
            };
            let nodes_before = self.nodes;

            if !root && best_score > -MATE_BOUND && board.has_non_pawn_material(board.stm) {
                let lmr_depth =
                    (depth - 1 - self.lmr[0][depth.min(63) as usize][move_count.min(63)]
                        + hist / p.hist_div)
                        .max(0);
                if quiet {
                    if move_count as i32 >= lmp_limit {
                        picker.skip_quiets = true;
                        continue;
                    }
                    if lmr_depth < 4 && hist < -p.hist_prune * depth {
                        continue;
                    }
                    if !in_check
                        && lmr_depth < 9
                        && static_eval + p.futility_base + p.futility_mult * lmr_depth <= alpha
                    {
                        picker.skip_quiets = true;
                        continue;
                    }
                    if !board.see_ge(mv, -p.see_quiet * lmr_depth * lmr_depth) {
                        continue;
                    }
                } else if depth <= 8 && !board.see_ge(mv, -p.see_noisy * depth) {
                    continue;
                }
            }

            // Singular extension.
            let mut ext = 0;
            if !root
                && depth >= p.singular_depth
                && mv == tt_move
                && excluded.is_none()
                && tt_depth >= depth - 3
                && tt_bound & BOUND_LOWER != 0
                && tt_score.abs() < MATE_BOUND
                && (ply as i32) < 2 * self.root_depth
            {
                let s_beta = tt_score - depth * (1 + i32::from(tt_pv && !pv_node));
                let s_depth = (depth - 1) / 2;
                self.stack[ply].excluded = mv;
                let v = self.negamax(shared, board, s_beta - 1, s_beta, s_depth, ply, cut_node);
                self.stack[ply].excluded = Move::NONE;
                if self.stopped {
                    return 0;
                }
                if v < s_beta {
                    ext = 1;
                    if !pv_node && v < s_beta - 20 && self.stack[ply].double_ext <= 8 {
                        ext = 2;
                        self.stack[ply + 2].double_ext = self.stack[ply].double_ext + 1;
                    }
                } else if s_beta >= beta {
                    return s_beta;
                } else if tt_score >= beta {
                    ext = -2 + i32::from(pv_node);
                } else if cut_node {
                    ext = -1;
                }
            }

            let child = board.make_move(mv);
            shared.tt.prefetch(child.hash);
            self.push_move(board, &child, mv, ply);
            let gives_check = child.in_check();
            if ext == 0
                && p.x_checkext != 0
                && gives_check
                && depth > 9
                && static_eval != NO_SCORE
                && static_eval.abs() > 45
            {
                ext = 1;
            }
            let new_depth = depth - 1 + ext;
            let mut score = 0;
            if depth >= 2 && move_count > 1 + usize::from(root) && (quiet || !tt_pv) {
                let killer_or_counter = mv == self.histories.killers[ply][0]
                    || mv == self.histories.killers[ply][1]
                    || mv == counter;
                let r = if p.x_frac != 0 {
                    // Reductions in 1/1024 plies with a continuous history term.
                    let mut r =
                        self.lmr_f[usize::from(!quiet)][depth.min(63) as usize][move_count.min(63)];
                    r += 1024 * i32::from(!pv_node);
                    r -= 1024 * i32::from(tt_pv);
                    r += 2048 * i32::from(cut_node);
                    r -= 1024 * i32::from(gives_check);
                    r += 1024 * i32::from(tt_noisy);
                    r += 1024 * i32::from(!improving);
                    r -= hist * 1024 / p.hist_div;
                    if p.x_cutoff != 0 && self.stack[ply + 1].cutoff_cnt > 3 {
                        r += 1024;
                    }
                    if killer_or_counter {
                        r -= 1024;
                    }
                    (r + 512).div_euclid(1024)
                } else {
                    let mut r =
                        self.lmr[usize::from(!quiet)][depth.min(63) as usize][move_count.min(63)];
                    r += i32::from(!pv_node);
                    r -= i32::from(tt_pv);
                    r += 2 * i32::from(cut_node);
                    r -= i32::from(gives_check);
                    r += i32::from(tt_noisy);
                    r += i32::from(!improving);
                    r -= hist / p.hist_div;
                    if p.x_cutoff != 0 && self.stack[ply + 1].cutoff_cnt > 3 {
                        r += 1;
                    }
                    if killer_or_counter {
                        r -= 1;
                    }
                    r
                };
                let d = (new_depth - r).clamp(1, new_depth.max(1));
                self.stack[ply].reduction = new_depth - d;
                score = -self.negamax(shared, &child, -alpha - 1, -alpha, d, ply + 1, true);
                self.stack[ply].reduction = 0;
                if score > alpha && d < new_depth {
                    let mut full = new_depth;
                    if p.x_deeper != 0 {
                        if score > best_score + 40 + 2 * new_depth {
                            full += 1;
                        } else if score < best_score + 8 {
                            full -= 1;
                        }
                    }
                    if full > d {
                        score = -self.negamax(
                            shared,
                            &child,
                            -alpha - 1,
                            -alpha,
                            full,
                            ply + 1,
                            !cut_node,
                        );
                        if p.x_postlmr != 0 && quiet && !self.stopped {
                            let b = if score > alpha {
                                history_bonus(full)
                            } else {
                                -history_malus(full, p.x_malus != 0)
                            };
                            self.update_cont(
                                ply,
                                board.mailbox[mv.from()] as usize * 64 + mv.to(),
                                b,
                            );
                        }
                    }
                }
            } else if !pv_node || move_count > 1 {
                score = -self.negamax(
                    shared,
                    &child,
                    -alpha - 1,
                    -alpha,
                    new_depth,
                    ply + 1,
                    !cut_node,
                );
            }
            if pv_node && (move_count == 1 || score > alpha) {
                score = -self.negamax(shared, &child, -beta, -alpha, new_depth, ply + 1, false);
            }
            self.pop_move();
            if self.stopped {
                return 0;
            }
            if root {
                self.root_nodes[mv.from()][mv.to()] += self.nodes - nodes_before;
            }
            if score > best_score {
                best_score = score;
                if score > alpha {
                    best_move = mv;
                    if pv_node {
                        self.update_pv(ply, mv);
                    }
                    if root {
                        self.root_best = mv;
                    }
                    if score >= beta {
                        self.stack[ply].cutoff_cnt += 1 + i32::from(tt_move.is_none());
                        break;
                    }
                    alpha = score;
                }
            }
            if quiet {
                if quiet_count < quiets.len() {
                    quiets[quiet_count] = mv;
                    quiet_count += 1;
                }
            } else if noisy_count < noisies.len() {
                noisies[noisy_count] = mv;
                noisy_count += 1;
            }
        }

        if move_count == 0 {
            return if !excluded.is_none() {
                alpha
            } else if in_check {
                -MATE + ply as i32
            } else {
                0
            };
        }

        if best_score >= beta {
            let bonus_depth = depth + i32::from(best_score > beta + 80);
            if !best_move.is_noisy() {
                self.update_quiet_stats(board, ply, best_move, &quiets[..quiet_count], bonus_depth);
            } else {
                self.update_capture(board, best_move, history_bonus(bonus_depth));
            }
            let malus = history_malus(bonus_depth, p.x_malus != 0);
            for &mv in &noisies[..noisy_count] {
                self.update_capture(board, mv, -malus);
            }
        } else if p.x_prior != 0 && best_move.is_none() && ply >= 1 && excluded.is_none() {
            // Fail low: the opponent's previous quiet move refuted us; reward it.
            let prev = self.stack[ply - 1].mv;
            if !prev.is_none() && !prev.is_noisy() {
                let scale = 1
                    + i32::from(depth > 5)
                    + i32::from(pv_node || cut_node)
                    + i32::from(best_score < alpha - 100);
                let bonus = history_bonus(depth) * scale / 2;
                gravity(
                    &mut self.histories.main[board.stm ^ 1][prev.from()][prev.to()],
                    bonus,
                );
                if let Some(c) = self.stack[ply - 1].cont {
                    self.update_cont(ply - 1, c, bonus);
                }
            }
        }

        if excluded.is_none() {
            let bound = if best_score >= beta {
                BOUND_LOWER
            } else if pv_node && best_score > original_alpha {
                BOUND_EXACT
            } else {
                BOUND_UPPER
            };
            if !in_check
                && (best_move.is_none() || !best_move.is_noisy())
                && !(bound == BOUND_LOWER && best_score <= static_eval)
                && !(bound == BOUND_UPPER && best_score >= static_eval)
            {
                self.update_correction(board, depth, best_score - static_eval, ply);
            }
            shared.tt.store(
                board.hash,
                best_move,
                score_to_tt(best_score, ply),
                raw_eval,
                depth,
                bound,
                tt_pv,
            );
        }
        best_score
    }

    /// Continuation-history update for move `idx` (`piece*64+to`) played at `ply`.
    fn update_cont(&mut self, ply: usize, idx: usize, bonus: i32) {
        for back in [1, 2, 4] {
            if ply >= back {
                if let Some(c) = self.stack[ply - back].cont {
                    gravity(&mut self.histories.cont[c][idx], bonus);
                }
            }
        }
    }

    fn update_capture(&mut self, board: &Board, mv: Move, bonus: i32) {
        let piece = board.mailbox[mv.from()] as usize;
        let victim = board.piece_on(mv.to()).unwrap_or(PAWN);
        gravity(&mut self.histories.capture[piece][mv.to()][victim], bonus);
    }

    fn update_quiet_stats(
        &mut self,
        board: &Board,
        ply: usize,
        best: Move,
        quiets: &[Move],
        depth: i32,
    ) {
        let bonus = history_bonus(depth);
        let stm = board.stm;
        let killers = &mut self.histories.killers[ply];
        if killers[0] != best {
            killers[1] = killers[0];
            killers[0] = best;
        }
        if ply >= 1 {
            if let Some(c) = self.stack[ply - 1].cont {
                self.histories.counter[c / 64][c % 64] = best;
            }
        }
        let conts = [
            if ply >= 1 {
                self.stack[ply - 1].cont
            } else {
                None
            },
            if ply >= 2 {
                self.stack[ply - 2].cont
            } else {
                None
            },
            if ply >= 4 {
                self.stack[ply - 4].cont
            } else {
                None
            },
        ];
        let pawn = (self.params.x_pawnhist != 0).then(|| pawn_index(board));
        let apply = |h: &mut Histories, mv: Move, b: i32| {
            gravity(&mut h.main[stm][mv.from()][mv.to()], b);
            let piece = board.mailbox[mv.from()] as usize;
            let idx = piece * 64 + mv.to();
            for c in conts.iter().flatten() {
                gravity(&mut h.cont[*c][idx], b);
            }
            if let Some(pi) = pawn {
                gravity(&mut h.pawn[pi][piece][mv.to()], b);
            }
        };
        apply(&mut self.histories, best, bonus);
        let malus = history_malus(depth, self.params.x_malus != 0);
        for &mv in quiets {
            if mv != best {
                apply(&mut self.histories, mv, -malus);
            }
        }
    }

    fn report(&self, shared: &Shared, depth: i32, score: i32, alpha: i32, beta: i32) {
        if self.silent {
            return;
        }
        let elapsed = self.start.elapsed().as_millis().max(1) as u64;
        let nodes = shared.nodes.load(Ordering::Relaxed) + (self.nodes - self.flushed_nodes);
        let bound = if score >= beta {
            " lowerbound"
        } else if score <= alpha {
            " upperbound"
        } else {
            ""
        };
        let pv: Vec<String> = self.pv[0][..self.pv_len[0]]
            .iter()
            .map(|m| m.to_uci())
            .collect();
        println!(
            "info depth {depth} seldepth {} score {}{bound} nodes {nodes} nps {} hashfull {} time {elapsed} pv {}",
            self.seldepth,
            format_score(score),
            nodes * 1000 / elapsed,
            shared.tt.hashfull(),
            pv.join(" ")
        );
    }

    /// Iterative deepening driver. `history` holds hashes of earlier game
    /// positions (oldest first) for repetition detection.
    pub fn iterate(
        &mut self,
        shared: &Shared,
        root: &Board,
        history: &[u64],
        limits: Limits,
        start: Instant,
    ) -> SearchResult {
        self.limits = limits;
        self.start = start;
        self.nodes = 0;
        self.flushed_nodes = 0;
        self.stopped = false;
        self.keys.clear();
        self.keys.extend_from_slice(history);
        for e in self.stack.iter_mut() {
            *e = StackEntry::default();
        }
        for row in self.root_nodes.iter_mut() {
            row.fill(0);
        }
        if let Some(net) = &self.net {
            self.acc.refresh(net, root, 0);
        }
        let legal = legal_moves(root);
        let mut result = SearchResult {
            best: legal.first().copied().unwrap_or(Move::NONE),
            ..Default::default()
        };
        if legal.is_empty() {
            result.score = if root.in_check() { -MATE } else { 0 };
            return result;
        }
        self.root_best = Move::NONE;
        let max_depth = limits
            .depth
            .unwrap_or(MAX_PLY as i32 - 4)
            .clamp(1, MAX_PLY as i32 - 4);
        let mut score = 0;
        let mut stability = 0;
        let mut prev_best = Move::NONE;
        let mut scores: Vec<i32> = Vec::new();
        for depth in 1..=max_depth {
            self.root_depth = depth;
            self.seldepth = 0;
            if self.params.x_optimism != 0 && depth >= 4 {
                let o = self.params.x_optimism * score / (score.abs() + 90);
                self.optimism[root.stm] = o;
                self.optimism[root.stm ^ 1] = -o;
            } else {
                self.optimism = [0; 2];
            }
            let mut delta = self.params.asp_delta + score * score / 16_000;
            let (mut alpha, mut beta) = if depth >= 4 {
                ((score - delta).max(-INF), (score + delta).min(INF))
            } else {
                (-INF, INF)
            };
            let mut search_depth = depth;
            loop {
                let v = self.negamax(shared, root, alpha, beta, search_depth.max(1), 0, false);
                if self.stopped {
                    break;
                }
                if v <= alpha {
                    beta = (alpha + beta) / 2;
                    alpha = (v - delta).max(-INF);
                    search_depth = depth;
                    if self.id == 0 && self.start.elapsed().as_millis() > 1000 {
                        self.report(shared, depth, v, alpha, beta);
                    }
                } else if v >= beta {
                    beta = (v + delta).min(INF);
                    if v.abs() < MATE_BOUND {
                        search_depth = (search_depth - 1).max(depth - 3);
                    }
                    if self.id == 0 && self.start.elapsed().as_millis() > 1000 {
                        self.report(shared, depth, v, alpha, beta);
                    }
                } else {
                    score = v;
                    break;
                }
                delta += delta / 2;
            }
            if self.stopped && depth > 1 {
                break;
            }
            if self.pv_len[0] > 0 {
                result.best = self.pv[0][0];
                result.ponder = if self.pv_len[0] > 1 {
                    self.pv[0][1]
                } else {
                    Move::NONE
                };
                result.pv = self.pv[0][..self.pv_len[0]].to_vec();
            }
            result.score = score;
            result.depth = depth;
            self.root_best = result.best;
            if self.id == 0 {
                self.report(shared, depth, score, -INF, INF);
            }
            if self.stopped {
                break;
            }
            if self.id != 0 {
                continue;
            }
            // Soft limits between iterations.
            scores.push(score);
            if result.best == prev_best {
                stability = (stability + 1).min(6);
            } else {
                stability = 0;
            }
            prev_best = result.best;
            if let Some(soft) = limits.soft_nodes {
                if self.nodes >= soft {
                    break;
                }
            }
            if let Some(soft) = limits.soft_ms {
                let total: u64 = self.root_nodes.iter().flatten().sum::<u64>().max(1);
                let best_nodes = self.root_nodes[result.best.from()][result.best.to()];
                let fraction = best_nodes as f64 / total as f64;
                let node_scale = (1.5 - fraction) * 1.35;
                let stab_scale = [2.2, 1.6, 1.3, 1.1, 1.0, 0.9, 0.8][stability];
                let mut target =
                    soft as f64 * node_scale * stab_scale * f64::from(self.params.tm_pct) / 100.0;
                if self.params.x_tm != 0 && scores.len() >= 4 {
                    // Spend more when the score is falling, less when rising.
                    let prev = scores[scores.len() - 4];
                    let fall = f64::from((prev - score).clamp(-100, 200));
                    target *= (1.0 + fall / 250.0).clamp(0.8, 1.6);
                }
                let elapsed = self.start.elapsed().as_millis() as f64;
                if elapsed >= target {
                    break;
                }
            }
        }
        shared
            .nodes
            .fetch_add(self.nodes - self.flushed_nodes, Ordering::Relaxed);
        self.flushed_nodes = self.nodes;
        result.nodes = shared.nodes.load(Ordering::Relaxed);
        result
    }
}

/// Run a search with `threads` (the first of which is the main thread).
pub fn search(
    shared: &Shared,
    threads: &mut [Thread],
    root: &Board,
    history: &[u64],
    limits: Limits,
) -> SearchResult {
    let start = Instant::now();
    shared.stop.store(false, Ordering::Relaxed);
    shared.nodes.store(0, Ordering::Relaxed);
    shared.tt.new_search();
    let (main, helpers) = threads.split_first_mut().expect("at least one thread");
    std::thread::scope(|scope| {
        for helper in helpers.iter_mut() {
            helper.silent = true;
            let mut helper_limits = limits;
            helper_limits.soft_ms = None;
            helper_limits.hard_ms = None;
            helper_limits.soft_nodes = None;
            scope.spawn(move || {
                helper.iterate(shared, root, history, helper_limits, start);
            });
        }
        let result = main.iterate(shared, root, history, limits, start);
        shared.stop.store(true, Ordering::Relaxed);
        result
    })
}

#[allow(dead_code)]
fn _assert_send() {
    fn is_send<T: Send>() {}
    is_send::<Thread>();
    let _ = MoveList::new();
}
